---
title: Files and options
description: The options screen, where Sigilfall keeps its save and options, and the environment variables it reads.
---

## Options

The **Options** screen of the title has five settings. Each applies at once
and is kept for the next sessions.

| Option | Values |
|---|---|
| **Glyphs** | Unicode or ASCII |
| **Colours** | Auto, 16 colours or Monochrome |
| **Theme** | Dark, or Light for 24-bit colour terminals |
| **Language** | English or Français (see [Language](/sigilfall/language/)) |
| **Keyboard** | QWERTY or AZERTY (see [Keyboard layouts](/sigilfall/keyboard/)) |

Auto follows the terminal and `NO_COLOR`; 16 colours and Monochrome apply
whatever they say. Everything on screen also reads without colour: each
piece of information has a glyph, a word or a prefix as well.

## The data folder

The game keeps two files in the data folder of your system:

| System | Folder |
|---|---|
| Windows | `%APPDATA%\Sigilfall\data` |
| Linux | `$XDG_DATA_HOME/sigilfall`, by default `~/.local/share/sigilfall` |
| macOS | `~/Library/Application Support/Sigilfall` |

- `save.ron` holds the game in progress. It is written when you quit and
  deleted when you die (death is final) or break the seal. A save that cannot
  be read back (damaged, or written by another version) is renamed
  `save.ron.bad`, the title screen says why, and a new game starts.
- `options.ron` holds the options. If it cannot be read, the default options
  are used and the file is replaced at the next change.

Both are readable RON text.

## Environment variables

| Variable | Effect |
|---|---|
| `SIGILFALL_DATA_DIR` | Use this folder instead of the system's data folder, for the save and the options. Created if missing; ignored when empty. |
| `NO_COLOR` | When set to a non-empty value and **Colours** is Auto, draw without colour ([no-color.org](https://no-color.org)). Choosing 16 colours in the options wins over it. |
| `COLORTERM` | `truecolor` or `24bit` turns on 24-bit colour; otherwise the 16 ANSI colours are used, except in Windows Terminal (detected by `WT_SESSION`), which gets 24-bit colour. |

For example, to play with a separate save and options, on Linux or macOS:

```sh
SIGILFALL_DATA_DIR=/tmp/sigilfall-test sigilfall
```

## If something goes wrong

- **The screen asks for a larger terminal.** The game needs at least 80 × 24
  cells; enlarge the window or reduce the font size.
- **Some characters show as boxes or take two cells.** The font lacks the
  game's glyphs: choose ASCII in **Glyphs**, or start with `--ascii`.
- **The title screen says the save could not be read.** The save was
  renamed `save.ron.bad` in the data folder and a new game starts. The
  message says why, cause first.
- **The colours are hard to read.** Try the other **Theme**, or
  **Monochrome** in **Colours**.
