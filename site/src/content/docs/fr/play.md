---
title: Comment jouer
description: L'écran titre, la ligne de commande, se déplacer et combattre, tracer des sigilles, descendre l'escalier et sauvegarder.
---

Cette page nomme les touches d'un clavier QWERTY. En AZERTY, certaines
changent : [Dispositions de clavier](/sigilfall/fr/keyboard/) donne les deux.

## L'écran titre

```sh
sigilfall
```

L'écran titre propose **Continuer** (quand une partie est sauvegardée),
**Nouvelle partie**, **Graine** (tapez la graine de la nouvelle partie),
**Donjon du jour** (celui de la date UTC du jour, affichée à côté),
**Options** et **Quitter**.

Une partie du jour affiche sa date dans la ligne d'état à la place de la
graine, et la garde une fois sauvegardée puis reprise, même un autre jour.

## Ligne de commande

| Option | Effet |
|---|---|
| `--seed <SEED>` | Lance tout de suite une partie de cette graine, sans l'écran titre. La graine est un nombre comme `42`, ou la forme que le jeu affiche, comme `7F3A-91C2-0B5E-D4A8`. Refusé tant qu'une partie est sauvegardée, pour ne jamais la perdre : continuez-la, ou abandonnez-la depuis l'écran titre, d'abord. |
| `--daily` | Lance tout de suite le donjon du jour, sans l'écran titre : le même donjon pour tous les joueurs de la même version du jeu à la même date UTC, un nouveau chaque jour à minuit UTC. Incompatible avec `--seed`. Refusé tant qu'une partie est sauvegardée, comme `--seed`. |
| `--ascii` | Dessine avec de simples caractères ASCII au lieu des glyphes Unicode, pour cette session seulement, quoi que dise l'option Glyphes. |
| `--lang <LANG>` | Joue en `en` (anglais) ou en `fr` (français) pour cette session seulement. Voir [Langue](/sigilfall/fr/language/). |
| `--keyboard <LAYOUT>` | Prend les touches d'un clavier `qwerty` ou `azerty` pour cette session seulement. Voir [Dispositions de clavier](/sigilfall/fr/keyboard/). |
| `--help`, `--version` | Affiche l'aide ou la version. |

Par exemple, pour rejouer un donjon partagé :

```sh
sigilfall --seed 7F3A-91C2-0B5E-D4A8
```

## Se déplacer et combattre

Déplacez-vous d'une case à la fois, diagonales comprises, avec les flèches,
`w a s d`, les chiffres `1`-`9` autour de `5` (pavé numérique ou rangée du
haut) ou les touches vi `h j k l y u b n`. `.` ou `5` fait attendre un tour.

Aller sur un monstre l'attaque. Un coup touche toujours, et inflige
l'attaque de l'attaquant moins la défense du défenseur, plus un tirage entre
−1 et +1, avec au moins 1 point de dégâts.

Le temps se compte en énergie. Chaque acteur gagne sa vitesse en énergie à
chaque tic et agit dès qu'il en a 100 ; chaque action a son propre coût. Un
monstre deux fois plus rapide que vous agit deux fois par pas que vous
faites.

Le panneau latéral montre votre santé et votre essence, les sigilles que
vous connaissez, et ce qui est en vue, avec les points de vie actuels et
maximums de chaque monstre.

### Explorer

`x` marche vers l'endroit inexploré le plus proche, un pas à la fois, chacun
un déplacement normal. L'exploration s'arrête dès qu'un monstre entre en
vue, qu'un objet ou un glyphe non lu apparaît, que vous êtes blessé, qu'il
ne reste rien à explorer, qu'une touche est pressée, ou au bout de 200 pas ;
le journal dit pourquoi. Elle ne démarre pas avec un monstre en vue.

### Objets

`g` ou `,` ramasse l'objet à vos pieds, et `i` ouvre l'inventaire, qui
contient 10 objets. Dans l'inventaire, Haut et Bas (ou `k` et `j`, `w` et
`s`), Origine et Fin, ou les chiffres `1`-`9` et `0` choisissent un objet ;
Entrée ou `u` l'utilise, `d` le pose, Échap ferme l'inventaire.

## Tracer des sigilles

Chaque partie commence avec Braise (`7 5 3`), un sort qui brûle l'ennemi le
plus proche ; la première ligne du journal le dit, avec la touche qui ouvre
la grille.

Appuyez sur `t` pour ouvrir la grille. Elle est numérotée comme le pavé
numérique :

```text
7 8 9
4 5 6
1 2 3
```

| Touches | Action |
|---|---|
| `1`-`9`, ou le pavé numérique avec ou sans Verr Num | Relier ce nœud au tracé |
| Retour arrière | Retirer le dernier nœud |
| Entrée | Lancer le tracé |
| Échap | Fermer la grille ; le temps ne passe pas |

Un sigille visé demande ensuite une direction : une touche de déplacement
vise, Échap revient au tracé. Après un sort instable ou raté, n'importe
quelle touche ferme le verdict.

Ce que fait le tracé :

- Un tracé qui reproduit exactement un motif connu le lance.
- À un nœud près, le sort part **instable** et un contrecoup suit.
- Tout le reste se **dissipe**. Un motif qui ne ressemble à aucun de ceux
  que vous connaissez est signalé comme absent de votre grimoire.

Chaque sort coûte 2 d'essence, sur 6 au plus, et l'essence revient
lentement. Un sort prend un demi-tour, plus un quart de tour par nœud
relié, et les monstres continuent de bouger pendant ce temps.

Les glyphes gravés dans le sol (`§`) enseignent les autres sigilles quand
vous marchez dessus. Le grimoire à côté de la grille liste les motifs que
vous connaissez.

## Descendre

Debout sur l'escalier, appuyez sur `>` ou `E` pour descendre à l'étage
suivant. Vous gardez vos points de vie, votre essence, votre inventaire et
votre grimoire.

Le dixième étage n'a pas d'escalier mais un sceau, `◎` sur la carte (`0` en
ASCII). Debout dessus, les mêmes touches le brisent et gagnent la partie.

## Sauvegarder et quitter

- `q` sauvegarde et quitte depuis la carte.
- Ctrl+C quitte depuis n'importe quel écran, en sauvegardant la partie en
  cours.
- Échap ne quitte jamais : il ferme la grille, l'inventaire et les
  sous-écrans du titre, et ne fait rien sur la carte.

Au lancement suivant, **Continuer** reprend la partie sauvegardée, qui se
déroule exactement comme sans avoir quitté. La mort est définitive : la
sauvegarde est supprimée quand vous mourez, et quand vous brisez le sceau.
[Fichiers et options](/sigilfall/fr/files/) dit où la sauvegarde est gardée.

## La fin d'une partie

Après la mort, le récapitulatif montre la graine de la partie et une ligne à
copier et partager, comme :

```text
Sigilfall daily 2026-11-08 - depth 4, turn 812, 23 slain, 9 sigils
```

ou `Sigilfall seed 7F3A-91C2-0B5E-D4A8 - …` pour une partie d'une graine.
Briser le sceau ouvre l'écran de victoire, avec le même récapitulatif et une
ligne qui dit `victory` à la place de la profondeur :

```text
Sigilfall daily 2026-11-08 - victory, turn 4120, 96 slain, 31 sigils
```

Sur ces deux écrans, Entrée revient au titre et `q` quitte. Pendant une
seconde après la fin de la partie, seul Ctrl+C fonctionne, pour qu'une
touche déjà en route ne ferme pas le récapitulatif avant qu'on l'ait vu.
