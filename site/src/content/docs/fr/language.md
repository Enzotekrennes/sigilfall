---
title: Langue
description: Jouer à Sigilfall en anglais ou en français, avec l'option Langue ou l'option --lang de la ligne de commande.
---

Tout le jeu est écrit en anglais et en français : menus, messages, noms des
créatures, des objets et des sigilles, et chaque écran.

## Choisir la langue

- Dans le jeu, **Options** › **Langue** règle English ou Français. Le choix
  s'applique tout de suite et est gardé pour les sessions suivantes.
- En ligne de commande, `--lang` règle la langue pour une session seulement,
  quoi que dise l'option :

  ```sh
  sigilfall --lang fr
  sigilfall --lang en
  ```

Tant que vous ne choisissez pas de langue, le jeu parle français sur un
système réglé en français, et anglais sinon.

## Ce qui reste en anglais

- La ligne à partager des écrans de fin, comme
  `Sigilfall seed 7F3A-91C2-0B5E-D4A8 - depth 4, turn 812, 23 slain, 9 sigils`,
  pour que les lignes de deux joueurs se comparent quelle que soit leur
  langue. La graine est aussi la même dans les deux langues.
- `--help`, comme pour la plupart des outils en ligne de commande, et les
  erreurs écrites sur la sortie d'erreur.

## Langue et clavier

La langue et la disposition de clavier sont deux options distinctes. Tant
qu'aucun clavier n'a été choisi, le jeu le devine d'après la langue et la
région du système : voir [Dispositions de clavier](/sigilfall/fr/keyboard/).

Chaque écran tient en 80 × 24 en anglais et en français, en QWERTY comme en
AZERTY.
