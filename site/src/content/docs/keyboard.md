---
title: Keyboard layouts
description: The keys of Sigilfall on QWERTY and AZERTY keyboards, the Keyboard option and the --keyboard command-line option.
---

The keys follow the keyboard layout chosen in the options, QWERTY or AZERTY,
so that moving and tracing never need Shift.

## Choosing the layout

- In the game, **Options** › **Keyboard** sets QWERTY or AZERTY. The choice
  applies at once and is kept for the next sessions.
- On the command line, `--keyboard` sets the layout for one session only,
  whatever the option says:

  ```sh
  sigilfall --keyboard azerty
  sigilfall --keyboard qwerty
  ```

Until you choose a keyboard, the game takes AZERTY on a system set to the
French of France or Belgium, where AZERTY keyboards are the usual ones, and
QWERTY otherwise. The layout itself cannot be read from every system and
terminal in a safe and reliable way, so the game guesses from the language
and region of the system, and the option sets it right.

## Keys shared by both layouts

Both layouts share the arrows, the keypad (with or without Num Lock), the
digits and the vi keys `h j k l y u b n`. QWERTY adds WASD. AZERTY adds ZQSD
and the top row typed without Shift, `& é " ' ( - è _ ç à`, which stands for
the digits `1` to `0` wherever a digit counts. With Shift, the same keys give
the digits, which work too.

## On the map

| Action | QWERTY | AZERTY |
|---|---|---|
| Move one cell, diagonals included; moving into a monster attacks it | Arrows, `w a s d`, `1`-`9` around `5` (keypad or top row), `h j k l y u b n` | Arrows, `z q s d`, `1`-`9` around `5` (keypad, or top row with or without Shift: `& é " ' - è _ ç`), `h j k l y u b n` |
| Move diagonally (the keypad without Num Lock) | Home, Page Up, End, Page Down | Home, Page Up, End, Page Down |
| Wait a turn | `.`, `5`, keypad centre | `.`, `5` or `(`, keypad centre |
| Open the sigil grid to trace a sigil | `t` | `t` |
| Pick up the item underfoot | `g`, `,` | `g`, `,` |
| Open the inventory | `i` | `i` |
| Go down the stairs, when standing on them; on the tenth floor, break the seal standing on it | `>`, `E` | `>`, `<`, `E` |
| Explore: walk towards the nearest unexplored place, step by step | `x` | `x` |
| Save and quit | `q` | `p` |
| Quit from any screen, saving the game in progress | Ctrl+C | Ctrl+C |

On AZERTY, `q` and `d` move, so quitting takes `p`. On both layouts the
letter that goes down the stairs is `E`: `d` moves east, and `e` does nothing
on the map, so Caps Lock never turns a step into a descent. A movement letter
typed in upper case does nothing, and the log asks once whether Caps Lock is
on.

No key does two things on one screen of one layout; a test goes through
every key of every screen to keep it so.

## On the sigil grid

The grid is numbered like the keypad (7 8 9 on top, 1 2 3 at the bottom).
Each node shows its digit, as the grimoire writes the patterns, and on AZERTY
the key that links it under the digit (`è` under 7).

| Keys | Action |
|---|---|
| `1`-`9`, on AZERTY also the top row without Shift (`&` for `1` to `ç` for `9`), or the keypad with or without Num Lock | Link that node to the trace |
| Backspace | Remove the last node |
| Enter | Cast the trace |
| Escape | Close the grid; no time passes |

## In the inventory and the menus

- **Inventory**: Up and Down (or `k` and `j`, `w` and `s` on QWERTY, `z` and
  `s` on AZERTY), Home and End, or the digits `1`-`9` and `0` (on AZERTY also
  `&` to `à` without Shift) choose an item; Enter or `u` uses it, `d` drops
  it, Escape closes the inventory.
- **Title screen and confirmation**: Up and Down (or `k` and `j`, `w` and `s`
  on QWERTY, `z` and `s` on AZERTY), Home and End, Enter; Escape goes back
  from the confirmation, `q` quits from the title (`p` on AZERTY).
- **Seed entry**: Backspace erases, Enter starts, Escape goes back.
- **Options**: Up and Down (or `k` and `j`, `w` and `s`, `z` and `s`) choose;
  Left and Right (or `h` and `l`, `a` and `d` on QWERTY, `q` and `d` on
  AZERTY), Enter or Space change the value; Escape goes back.
- **End screens**: Enter goes back to the title, `q` quits (`p` on AZERTY).

## The key help

The key help at the bottom of the screen names the letters of your layout
for moving, the key of the stairs typed without Shift (`>` on QWERTY, `<` on
AZERTY), and says that quitting saves the game. It names `x` when the row has
room for it: from 80 columns in English, from 92 in French. The arrows, the
digits and the vi keys are listed on this page only.
