---
title: Installer
description: Installer Sigilfall avec les installateurs d'une version, depuis une archive, ou depuis les sources avec cargo.
---

Sigilfall tourne dans un terminal d'au moins 80 × 24 cellules, sous Windows,
Linux et macOS. Windows Terminal, ou tout terminal dont la police a les
caractères de dessin de cadres, l'affiche le mieux ; l'option `--ascii`
couvre les autres.

## Avec les installateurs

Chaque [version](https://github.com/Enzotekrennes/sigilfall/releases) propose
deux installateurs qui téléchargent la bonne archive pour votre système et
placent le programme `sigilfall` dans le dossier `bin` de votre dossier
Cargo (`~/.cargo/bin` par défaut).

Sous Linux et macOS :

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/Enzotekrennes/sigilfall/releases/latest/download/sigilfall-installer.sh | sh
```

Sous Windows, dans PowerShell :

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/Enzotekrennes/sigilfall/releases/latest/download/sigilfall-installer.ps1 | iex"
```

## Depuis une archive

Chaque version propose aussi une archive du jeu pour chaque système :

| Système | Archive |
|---|---|
| Windows (x86_64) | `sigilfall-x86_64-pc-windows-msvc.zip` |
| Linux (x86_64, glibc) | `sigilfall-x86_64-unknown-linux-gnu.tar.xz` |
| macOS (Intel) | `sigilfall-x86_64-apple-darwin.tar.xz` |
| macOS (Apple Silicon) | `sigilfall-aarch64-apple-darwin.tar.xz` |

Chaque archive a un fichier `.sha256` à côté pour vérifier le
téléchargement. Téléchargez l'archive de votre système depuis la
[page des versions](https://github.com/Enzotekrennes/sigilfall/releases),
extrayez-la, et lancez le programme `sigilfall` qu'elle contient depuis un
terminal.

## Depuis les sources

Avec Rust 1.88 ou plus récent :

```sh
cargo install --locked --git https://github.com/Enzotekrennes/sigilfall sigilfall
```

Le contenu du jeu (monstres, sigilles, objets, étages) est intégré au
programme : cette seule commande suffit, aucun dossier de données n'est à
copier.

## Lancer le jeu

```sh
sigilfall
```

La page [Comment jouer](/sigilfall/fr/play/) explique l'écran titre et les
options de la ligne de commande.
