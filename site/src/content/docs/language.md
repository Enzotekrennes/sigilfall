---
title: Language
description: Play Sigilfall in English or in French, with the Language option or the --lang command-line option.
---

The whole game is written in English and in French: menus, messages, the
names of creatures, items and sigils, and every screen.

## Choosing the language

- In the game, **Options** › **Language** sets English or Français. The
  choice applies at once and is kept for the next sessions.
- On the command line, `--lang` sets the language for one session only,
  whatever the option says:

  ```sh
  sigilfall --lang fr
  sigilfall --lang en
  ```

Until you choose a language, the game speaks French on a system set to French
and English otherwise.

## What stays in English

- The line to share on the end screens, such as
  `Sigilfall seed 7F3A-91C2-0B5E-D4A8 - depth 4, turn 812, 23 slain, 9 sigils`,
  so that two players' lines compare whatever their language. The seed is the
  same in both languages too.
- `--help`, as for most command-line tools, and the errors written on the
  error output.

## Language and keyboard

The language and the keyboard layout are two separate options. When no
keyboard has been chosen, the game guesses it from the language and region
of the system: see [Keyboard layouts](/sigilfall/keyboard/).

Every screen fits 80 × 24 in English and in French, on QWERTY and on AZERTY.
