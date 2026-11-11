---
title: Architecture
description: Les deux crates de Sigilfall, comment une touche devient un tour, comment un étage naît d'une graine, et les principales décisions de conception.
---

Cette page résume l'architecture. [ARCHITECTURE.md](https://github.com/Enzotekrennes/sigilfall/blob/main/ARCHITECTURE.md)
sur GitHub (en anglais) donne chaque module, les flux et toutes les
décisions de conception (D1 à D28) avec les solutions écartées.

## Deux crates

Sigilfall est un workspace Cargo à deux crates : les règles du jeu dans une
bibliothèque, et l'application en terminal dans un binaire qui en dépend.

| Crate | Type | Responsabilité |
|---|---|---|
| `crates/sigilfall-core` | bibliothèque | Logique du jeu : état du monde, ordre des tours, génération du donjon, combat, sigilles, objets, chargement du contenu, sauvegardes. |
| `crates/sigilfall` | binaire | Application en terminal : ligne de commande, dispositions de touches, écrans, rendu, fichiers gardés entre deux parties. |

- **`sigilfall-core` ne dépend d'aucune crate de terminal.** Elle ne dépend
  que de `rand`, `rand_chacha`, `serde`, `ron`, `slotmap` et `thiserror` :
  le compilateur vérifie ainsi que les règles ne dessinent ni ne lisent
  jamais une touche, et chaque règle se teste sans terminal.
- **Le cœur ne produit aucun texte.** `Game::submit` renvoie des `Event`,
  de simples données ; le binaire les change en phrases anglaises ou
  françaises. Les seuls mots que le cœur contient sont les noms du contenu,
  dans les deux langues.
- **Le binaire ne décide rien du jeu.** Il traduit les touches en `Action`,
  les soumet, et dessine l'état qu'il relit.

La [référence de sigilfall-core](/sigilfall/reference/core/) (en anglais)
décrit chaque module public de la bibliothèque.

## Une touche devient un tour

1. L'application dessine l'écran, puis attend une touche.
2. La table de touches de la disposition en cours change la touche en
   commande pour l'écran actif ; les relâchements, les répétitions et les
   touches sans rôle ne font rien.
3. Sur l'écran de jeu, une commande qui agit devient une `Action` et part
   vers `Game::submit`. Un sigille est d'abord dessiné dans la grille, puis
   soumis en entier, comme une seule `Action::Cast`.
4. `Game::submit` applique l'action du joueur. Une action que les règles
   refusent (un mur, pas d'escalier ici, pas assez d'essence) ne coûte rien.
   Sinon son coût est pris sur l'énergie du joueur et l'ordonnanceur
   tourne : à chaque tic, chaque acteur gagne sa vitesse en énergie, et un
   acteur qui en a 100 ou plus agit, les égalités allant au plus ancien.
5. Tout ce qui s'est passé revient sous forme d'une liste d'`Event`, qui
   alimente aussi les statistiques de la partie.
6. L'application change les événements en phrases, les ajoute au journal,
   passe à l'écran de fin si la partie est terminée, et l'image suivante
   dessine le nouvel état.

## Un étage à partir d'une graine

Une partie a une graine de 64 bits. Chaque étage et chaque usage (le type
d'étage, son plan, les combats) tire d'un flux ChaCha8 à lui, dérivé de
(graine, profondeur, usage). Un étage est donc une fonction pure de la
graine, de sa profondeur et du contenu, quoi qu'on ait joué au-dessus, et la
même graine donne le même donjon sur tous les systèmes.

- Le type d'étage est tiré d'abord, salles ou grottes, pondéré par
  `floors.ron` : les étages 1 et 2 sont toujours des salles, l'étage 10
  toujours une grotte.
- Les salles et les couloirs sont tracés par partition binaire de l'espace
  (BSP) ; les grottes poussent par un automate cellulaire et sont réduites à
  leur plus grande zone d'un seul tenant.
- L'escalier va sur la case la plus éloignée du départ ; à l'étage 10, le
  sceau prend sa place.
- Les monstres sont placés hors de vue du départ, puis les glyphes, puis les
  objets.

Le donjon du jour est la partie d'une graine calculée à partir de la date
UTC, sans aucun serveur.

## Contenu

Les monstres, les sigilles, les objets, les tables d'apparition et les
types d'étages vivent dans des fichiers RON de
`crates/sigilfall-core/content`. Ils sont intégrés au binaire et validés au
démarrage, avec des erreurs qui nomment le fichier, l'entrée, le champ et la
ligne. Un test vérifie que les fichiers livrés sont valides.

## Sauvegardes

Une sauvegarde est du texte RON lisible, avec une version de format. Elle
garde les flux aléatoires et l'ordre dans lequel les clés d'entités libérées
sont réutilisées : une partie reprise se déroule exactement comme sans avoir
quitté. Chaque valeur relue est vérifiée avant qu'on s'y fie ; une
sauvegarde fautive est refusée avec une raison au lieu d'échouer plus tard
en jeu.

## Principales décisions

| # | Choix | Écarté |
|---|---|---|
| D1 | Deux crates, bibliothèque du cœur et binaire en terminal, pour que le compilateur garantisse la frontière. | Une seule crate avec des modules. |
| D2 | Des structures simples : un `World` avec la carte et des slot maps d'acteurs et d'objets, à clés générationnelles et ordre déterministe. | Un ECS (bevy_ecs, hecs). |
| D3 | Un ordonnanceur à l'énergie : vitesse par tic, action à 100, un coût par action. | Une alternance joueur puis monstres. |
| D4 | `Game::submit(Action) -> Vec<Event>` : le cœur renvoie des données, le binaire écrit le texte. | Une logique de jeu qui écrit elle-même les messages. |
| D5 | Une graine `u64` et un flux aléatoire par (profondeur, usage). | Un seul générateur partagé. |
| D6 | BSP, remplissage par diffusion, cartes de Dijkstra, A* et shadowcasting symétrique écrits dans le projet, en arithmétique entière. | Des crates de recherche de chemin. |
| D7 | Des sigilles tracés sur une grille 3 × 3 de pavé numérique, avec un coût en temps par trait et des motifs appris sur des glyphes gravés. | Des sorts dans un menu, ou des parchemins à usage unique. |
| D8 | Un contenu intégré au binaire et validé au démarrage. | Un dossier de données lu à l'exécution. |
| D9 | `unsafe` interdit, documentation exigée sur chaque élément, avertissements de clippy et de rustdoc traités comme des erreurs. | Les lints par défaut. |
| D18 | Reconnaissance du tracé par distance d'édition ; motifs distants d'au moins 3 modifications, pour qu'un tracé à un nœud près ne corresponde qu'à un seul sigille. | Un écart de 2 modifications. |
| D21 | Des sauvegardes RON lisibles et versionnées, qui gardent le flux aléatoire exact et l'ordre de réutilisation des clés. | Un format binaire. |
| D27 | Chaque texte en anglais et en français, dans un catalogue dont le compilateur vérifie qu'il est complet. | Un framework de traduction et des fichiers lus à l'exécution. |
| D28 | Des tables de touches QWERTY et AZERTY, une par écran et par disposition, devinées d'après la langue du système tant qu'aucune n'est choisie. | Lire la disposition depuis le système, ce qui demande `unsafe` ou n'est pas fiable dans un terminal. |

## Identité visuelle

L'apparence du jeu vit dans le module `theme` du binaire : aucun écran
n'écrit de couleur ni de glyphe à lui.

- **Ambiance : le carnet d'un alchimiste lu à la bougie.** Fond d'encre,
  texte couleur d'os, or pour le joueur et ce qui compte, vert-de-gris pour
  tout ce qui touche aux sigilles, vermillon pour le danger.
- **Tout se lit sans couleur.** Chaque information a aussi un glyphe, un mot
  ou un préfixe.
- **Quatre modes de rendu** : 24 bits sombre, 24 bits clair (« parchemin »),
  les 16 couleurs ANSI, et monochrome.
- **Contraste** : chaque token porteur de sens atteint un ratio WCAG 2.x de
  4,5 sur les surfaces où il est dessiné, et un test recalcule chaque ratio.
