---
title: Contribuer
description: Compiler Sigilfall depuis les sources, lancer les tests et les lints, utiliser un dossier de données à part, et construire ce site de documentation.
---

## Prérequis

- Rust 1.88 ou plus récent. `rust-toolchain.toml` fixe le canal stable avec
  `clippy` et `rustfmt` ; `rustup toolchain install`, lancé dans le dépôt,
  les installe.
- Pour construire ce site : Node.js et npm.

## Compiler et lancer

```sh
cargo build --release --locked          # target/release/sigilfall
cargo run --locked -p sigilfall -- --help
```

## Tests et vérifications

Voici les vérifications que la CI lance sous Linux, Windows et macOS :

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --locked -- -D warnings   # the shipped code alone
cargo fmt --all --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
```

La CI vérifie aussi que le workspace compile avec Rust 1.88, et contrôle les
licences et les alertes de sécurité de chaque dépendance d'après
`deny.toml`.

Chaque élément du code est documenté : une documentation manquante est un
avertissement, et la CI change les avertissements en erreurs.

## Un dossier de données à part

`SIGILFALL_DATA_DIR` fait utiliser au jeu un autre dossier pour sa
sauvegarde et ses options, pour qu'une session de test ne touche jamais
votre propre partie :

```sh
SIGILFALL_DATA_DIR=./scratch-data cargo run --locked -p sigilfall
```

Le dossier est créé s'il manque ; une valeur vide est ignorée. Dans
PowerShell :

```powershell
$env:SIGILFALL_DATA_DIR = ".\scratch-data"; cargo run --locked -p sigilfall
```

## Modifier le contenu

Les monstres, les sigilles, les objets, les tables d'apparition et les types
d'étages sont des fichiers RON de `crates/sigilfall-core/content`, avec le
sens de chaque champ écrit en tête du fichier. Chaque entrée est nommée en
anglais et en français. Un test vérifie que les fichiers livrés sont
valides : `cargo test --workspace --locked` signale une entrée fautive avec
son fichier, son entrée, son champ et sa ligne.

## Versions

Les versions sont construites par
[`dist`](https://github.com/axodotdev/cargo-dist) à partir de
`dist-workspace.toml` : pousser un tag comme `v0.1.0` construit les binaires
et les installateurs, et les publie comme version GitHub.

## Ce site de documentation

Le site vit dans `site/` et est construit avec Starlight. Les pages
anglaises sont dans `site/src/content/docs/`, les pages françaises dans
`site/src/content/docs/fr/` avec les mêmes noms de fichiers, et les termes
employés en français dans `site/glossary.md`.

```sh
cd site
npm ci
npm run dev     # local preview
npm run build   # site/dist, with every internal link checked
```

La [référence de sigilfall-core](/sigilfall/reference/core/) (en anglais)
est générée à partir des commentaires `//!` de la crate par
`npm run docs:api`, que `dev` et `build` lancent d'abord : pour la modifier,
modifiez le commentaire Rust. La rustdoc complète est construite et copiée
sous `api/` par le workflow de documentation
(`.github/workflows/docs.yml`). Pour la voir en local :

```sh
cargo doc --no-deps -p sigilfall-core --locked
cp -r target/doc site/public/api
```
