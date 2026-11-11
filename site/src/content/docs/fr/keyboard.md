---
title: Dispositions de clavier
description: Les touches de Sigilfall sur un clavier QWERTY ou AZERTY, l'option Clavier et l'option --keyboard de la ligne de commande.
---

Les touches suivent la disposition de clavier choisie dans les options,
QWERTY ou AZERTY, pour que se déplacer et tracer ne demandent jamais Maj.

## Choisir la disposition

- Dans le jeu, **Options** › **Clavier** règle QWERTY ou AZERTY. Le choix
  s'applique tout de suite et est gardé pour les sessions suivantes.
- En ligne de commande, `--keyboard` règle la disposition pour une session
  seulement, quoi que dise l'option :

  ```sh
  sigilfall --keyboard azerty
  sigilfall --keyboard qwerty
  ```

Tant que vous ne choisissez pas de clavier, le jeu prend AZERTY sur un
système réglé en français de France ou de Belgique, où les claviers AZERTY
sont la norme, et QWERTY sinon. La disposition elle-même ne peut pas être
lue de façon sûre et fiable sur tous les systèmes et dans tous les
terminaux : le jeu la devine d'après la langue et la région du système, et
l'option la corrige.

## Touches communes aux deux dispositions

Les deux dispositions partagent les flèches, le pavé numérique (avec ou sans
Verr Num), les chiffres et les touches vi `h j k l y u b n`. QWERTY y ajoute
WASD. AZERTY y ajoute ZQSD et la rangée du haut tapée sans Maj,
`& é " ' ( - è _ ç à`, qui vaut les chiffres `1` à `0` partout où un chiffre
compte. Avec Maj, les mêmes touches donnent les chiffres, qui marchent aussi.

## Sur la carte

| Action | QWERTY | AZERTY |
|---|---|---|
| Se déplacer d'une case, diagonales comprises ; aller sur un monstre l'attaque | Flèches, `w a s d`, `1`-`9` autour de `5` (pavé numérique ou rangée du haut), `h j k l y u b n` | Flèches, `z q s d`, `1`-`9` autour de `5` (pavé numérique, ou rangée du haut avec ou sans Maj : `& é " ' - è _ ç`), `h j k l y u b n` |
| Se déplacer en diagonale (le pavé numérique sans Verr Num) | Origine, Page préc., Fin, Page suiv. | Origine, Page préc., Fin, Page suiv. |
| Attendre un tour | `.`, `5`, centre du pavé | `.`, `5` ou `(`, centre du pavé |
| Ouvrir la grille pour tracer un sigille | `t` | `t` |
| Ramasser l'objet à ses pieds | `g`, `,` | `g`, `,` |
| Ouvrir l'inventaire | `i` | `i` |
| Descendre l'escalier, debout dessus ; au dixième étage, briser le sceau, debout dessus | `>`, `E` | `>`, `<`, `E` |
| Explorer : marcher vers l'endroit inexploré le plus proche, pas à pas | `x` | `x` |
| Sauvegarder et quitter | `q` | `p` |
| Quitter depuis n'importe quel écran, en sauvegardant la partie en cours | Ctrl+C | Ctrl+C |

En AZERTY, `q` et `d` déplacent : quitter passe donc sur `p`. Sur les deux
dispositions, la lettre qui descend l'escalier est `E` (« escalier ») : `d`
déplace vers l'est et `e` ne fait rien sur la carte, si bien que Verr Maj ne
change jamais un pas en descente. Une lettre de déplacement tapée en
majuscule ne fait rien, et le journal demande une fois si Verr Maj est
activé.

Aucune touche ne fait deux choses sur un même écran d'une même disposition ;
un test passe par toutes les touches de tous les écrans pour que cela reste
vrai.

## Sur la grille des sigilles

La grille est numérotée comme le pavé numérique (7 8 9 en haut, 1 2 3 en
bas). Chaque nœud affiche son chiffre, comme le grimoire écrit les motifs,
et en AZERTY la touche qui le relie sous le chiffre (`è` sous le 7).

| Touches | Action |
|---|---|
| `1`-`9`, en AZERTY aussi la rangée du haut sans Maj (de `&` pour `1` à `ç` pour `9`), ou le pavé numérique avec ou sans Verr Num | Relier ce nœud au tracé |
| Retour arrière | Retirer le dernier nœud |
| Entrée | Lancer le tracé |
| Échap | Fermer la grille ; le temps ne passe pas |

## Dans l'inventaire et les menus

- **Inventaire** : Haut et Bas (ou `k` et `j`, `w` et `s` en QWERTY, `z` et
  `s` en AZERTY), Origine et Fin, ou les chiffres `1`-`9` et `0` (en AZERTY,
  aussi `&` à `à` sans Maj) choisissent un objet ; Entrée ou `u` l'utilise,
  `d` le pose, Échap ferme l'inventaire.
- **Écran titre et confirmation** : Haut et Bas (ou `k` et `j`, `w` et `s`
  en QWERTY, `z` et `s` en AZERTY), Origine et Fin, Entrée ; Échap revient
  de la confirmation, `q` quitte depuis le titre (`p` en AZERTY).
- **Saisie d'une graine** : Retour arrière efface, Entrée commence, Échap
  revient.
- **Options** : Haut et Bas (ou `k` et `j`, `w` et `s`, `z` et `s`)
  choisissent ; Gauche et Droite (ou `h` et `l`, `a` et `d` en QWERTY, `q` et
  `d` en AZERTY), Entrée ou Espace changent la valeur ; Échap revient.
- **Écrans de fin** : Entrée revient au titre, `q` quitte (`p` en AZERTY).

## L'aide des touches

L'aide des touches en bas de l'écran nomme les lettres de votre disposition
pour se déplacer, la touche de l'escalier tapée sans Maj (`>` en QWERTY, `<`
en AZERTY), et dit que quitter sauvegarde la partie. Elle nomme `x` quand la
ligne a la place : dès 80 colonnes en anglais, dès 92 en français. Les
flèches, les chiffres et les touches vi ne sont listés que sur cette page.
