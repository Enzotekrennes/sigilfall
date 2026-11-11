---
title: Install
description: Install Sigilfall with the release installers, from a release archive, or from source with cargo.
---

Sigilfall runs in a terminal of at least 80 × 24 cells, on Windows, Linux and
macOS. Windows Terminal, or any terminal with a font that has box drawing
characters, shows it best; the `--ascii` option covers the others.

## With the installers

Each [release](https://github.com/Enzotekrennes/sigilfall/releases) has two
installers that download the right archive for your system and put the
`sigilfall` program in the `bin` folder of your Cargo home (`~/.cargo/bin`
by default).

On Linux and macOS:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Enzotekrennes/sigilfall/releases/latest/download/sigilfall-installer.sh | sh
```

On Windows, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/Enzotekrennes/sigilfall/releases/latest/download/sigilfall-installer.ps1 | iex"
```

## From a release archive

Each release also has an archive of the game for each system:

| System | Archive |
|---|---|
| Windows (x86_64) | `sigilfall-x86_64-pc-windows-msvc.zip` |
| Linux (x86_64, glibc) | `sigilfall-x86_64-unknown-linux-gnu.tar.xz` |
| macOS (Intel) | `sigilfall-x86_64-apple-darwin.tar.xz` |
| macOS (Apple Silicon) | `sigilfall-aarch64-apple-darwin.tar.xz` |

Each archive has a `.sha256` file beside it to check the download. Download
the archive for your system from the
[releases page](https://github.com/Enzotekrennes/sigilfall/releases), extract
it, and run the `sigilfall` program it contains from a terminal.

## From source

With Rust 1.88 or later:

```sh
cargo install --locked --git https://github.com/Enzotekrennes/sigilfall sigilfall
```

The game's content (monsters, sigils, items, floors) is built into the
program, so this single command is enough: no data folder needs to be
copied.

## Start the game

```sh
sigilfall
```

The [How to play](/sigilfall/play/) page explains the title screen and the
command-line options.
