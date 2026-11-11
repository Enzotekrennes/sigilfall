---
title: Fichiers et options
description: L'écran des options, où Sigilfall garde sa sauvegarde et ses options, et les variables d'environnement qu'il lit.
---

## Options

L'écran **Options** du titre a cinq réglages. Chacun s'applique tout de
suite et est gardé pour les sessions suivantes.

| Option | Valeurs |
|---|---|
| **Glyphes** | Unicode ou ASCII |
| **Couleurs** | Auto, 16 couleurs ou Monochrome |
| **Thème** | Sombre, ou Clair pour les terminaux en couleurs 24 bits |
| **Langue** | English ou Français (voir [Langue](/sigilfall/fr/language/)) |
| **Clavier** | QWERTY ou AZERTY (voir [Dispositions de clavier](/sigilfall/fr/keyboard/)) |

Auto suit le terminal et `NO_COLOR` ; 16 couleurs et Monochrome
s'appliquent quoi qu'ils disent. Tout ce qui est à l'écran se lit aussi sans
couleur : chaque information a également un glyphe, un mot ou un préfixe.

## Le dossier de données

Le jeu garde deux fichiers dans le dossier de données de votre système :

| Système | Dossier |
|---|---|
| Windows | `%APPDATA%\Sigilfall\data` |
| Linux | `$XDG_DATA_HOME/sigilfall`, par défaut `~/.local/share/sigilfall` |
| macOS | `~/Library/Application Support/Sigilfall` |

- `save.ron` contient la partie en cours. Il est écrit quand vous quittez,
  et supprimé quand vous mourez (la mort est définitive) ou brisez le sceau.
  Une sauvegarde illisible (abîmée, ou écrite par une autre version) est
  renommée `save.ron.bad`, l'écran titre dit pourquoi, et une nouvelle
  partie commence.
- `options.ron` contient les options. S'il est illisible, les options par
  défaut s'appliquent et le fichier est remplacé au prochain changement.

Les deux sont du texte RON lisible.

## Variables d'environnement

| Variable | Effet |
|---|---|
| `SIGILFALL_DATA_DIR` | Utilise ce dossier au lieu du dossier de données du système, pour la sauvegarde et les options. Créé s'il manque ; ignoré s'il est vide. |
| `NO_COLOR` | Quand elle a une valeur non vide et que **Couleurs** vaut Auto, dessine sans couleur ([no-color.org](https://no-color.org)). Choisir 16 couleurs dans les options passe avant elle. |
| `COLORTERM` | `truecolor` ou `24bit` active les couleurs 24 bits ; sinon les 16 couleurs ANSI sont utilisées, sauf dans Windows Terminal (reconnu à `WT_SESSION`), qui a les couleurs 24 bits. |

Par exemple, pour jouer avec une sauvegarde et des options à part, sous
Linux ou macOS :

```sh
SIGILFALL_DATA_DIR=/tmp/sigilfall-test sigilfall
```

## En cas de problème

- **L'écran demande un terminal plus grand.** Le jeu demande au moins
  80 × 24 cellules : agrandissez la fenêtre ou réduisez la taille de la
  police.
- **Des caractères s'affichent en carrés ou prennent deux cellules.** La
  police n'a pas les glyphes du jeu : choisissez ASCII dans **Glyphes**, ou
  lancez le jeu avec `--ascii`.
- **L'écran titre dit que la sauvegarde n'a pas pu être lue.** La
  sauvegarde a été renommée `save.ron.bad` dans le dossier de données et une
  nouvelle partie commence. Le message dit pourquoi, la cause d'abord.
- **Les couleurs sont difficiles à lire.** Essayez l'autre **Thème**, ou
  **Monochrome** dans **Couleurs**.
