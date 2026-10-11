# BoomGround

Destruction de voxels façon Teardown, en Rust + Bevy. **Tout est destructible**
(sauf la couche de socle tout en bas) : immeubles, vitres, voitures, arbres, routes, sol.

## Installer (joueurs)

Il n'y a rien à compiler et aucun terminal à ouvrir.

| Système | Fichier | Ce qu'il fait |
|---|---|---|
| Windows | `BoomGround-Setup-Windows.exe` | Double-clic, puis « Oui ». Installe le jeu dans `%LOCALAPPDATA%\Programs\BoomGround` (sans droits administrateur), crée les raccourcis Bureau et menu Démarrer, et ajoute une entrée dans « Applications installées » pour désinstaller. |
| Linux | `BoomGround-Setup-Linux` | Double-clic (ou `./BoomGround-Setup-Linux`). Ajoute BoomGround au menu des applications et la commande `boomground`. |
| macOS | `BoomGround-Setup-macOS` | Clic droit puis **Ouvrir** la première fois (l'application n'est pas signée). Crée `~/Applications/BoomGround.app`. |

Des versions **portables** (`BoomGround-Windows.exe`, `BoomGround-Linux`, `BoomGround-macOS`)
se lancent aussi directement, sans installation : le jeu tient en un seul fichier, polices comprises.

Pour désinstaller : Windows → « Applications installées » → BoomGround ; Linux/macOS →
relancer l'installateur avec `--uninstall`.

## Modes de jeu

- **Bac à sable** : les 13 armes sont données d'office, munitions et bombes illimitées.
  `V` active le vol libre.
- **Mode normal** : on commence avec un simple marteau. Chaque voxel détruit rapporte
  de l'argent (brique 2 $, métal 3 $, verre 2 $, béton/bois/plâtre 1 $ ; le sol ne
  rapporte rien). La **boutique** (`B`) vend de meilleurs marteaux, des armes à feu,
  des munitions et des bombes, qui permettent de casser plus vite et donc de gagner plus.

## Carte et seed

La ville (153 × 153 m, 384 × 64 × 384 voxels) est **générée à partir d'une seed** :
rues, tours de bureaux, immeubles de briques avec escaliers praticables, entrepôts,
tours de verre, maisons, parcs, parkings avec voitures, chantiers.

- Dans le menu, la seed peut être un nombre ou un mot (`paris` donne toujours la même ville).
  « Hasard » en tire une nouvelle, et une seed laissée vide est tirée au hasard.
- En jeu : `F5` reconstruit la même ville, `F6` en génère une nouvelle.

## Arsenal

| Famille | Armes |
|---|---|
| Marteaux | Marteau, Masse, Masse de guerre, Marteau-piqueur (continu) |
| Armes à feu | Glock 17, Fusil à pompe (9 plombs), Fusil d'assaut (automatique), Sniper .50 (traverse les murs), Lance-roquettes |
| Bombes lancées | Grenade (rebondit), Dynamite (colle à la surface) |
| Bombes posées | C4 (clic droit pour toutes les faire sauter), Méga-bombe (minuterie de 8 s, rayon de 7 m) |

Les explosions s'enchaînent : une bombe prise dans le souffle d'une autre saute à son tour.

## Contrôles

| Touche | Action |
|---|---|
| ZQSD / WASD, flèches | se déplacer (les marches d'un voxel se montent sans sauter) |
| Maj | courir |
| Espace | sauter (en vol libre : monter ; Ctrl/C : descendre) |
| Souris | regarder (clic = capturer le curseur) |
| Clic gauche | agir avec l'arme en main |
| Clic droit | faire sauter les C4 posées |
| 1…9, 0, molette, Q / E | changer d'arme |
| R | recharger |
| B ou Tab | boutique (mode normal) |
| V | vol libre (bac à sable) |
| H | masquer l'aide |
| F5 / F6 | même ville / nouvelle ville |
| Échap | menu (pause) |

Les réglages (sensibilité, volume, limite d'images, axe Y inversé) sont enregistrés
dans `%APPDATA%\BoomGround` (Windows), `~/Library/Application Support/BoomGround`
(macOS) ou `~/.config/boomground` (Linux).

## Multijoueur

- **Héberger** : Menu → MULTIJOUEUR → HÉBERGER. Le serveur tourne dans le jeu lui-même,
  sans fenêtre ni terminal, sur le port **28431** avec la seed choisie.
- **Rejoindre** : Menu → MULTIJOUEUR, saisir `ip`, `ip:port` ou `domaine.tld`, puis REJOINDRE.
- **Serveur dédié** (facultatif) : `BoomGround-Server-<os>` (variables `BOOM_PORT`, `BOOM_SEED`).

Tout le monde reçoit la seed du serveur et l'historique des destructions : un joueur
qui arrive en cours de partie voit la ville dans son état actuel. Le serveur valide
chaque action (cadence, portée, rayon maximal par type d'arme, téléportation).
Le multijoueur se joue en bac à sable.

## Corrections de cette version

- **Collisions** : le joueur pouvait traverser les murs fins quand le jeu ralentissait,
  et se faisait téléporter de l'autre côté d'un cube quand il y entrait. Les déplacements
  sont désormais découpés en petits pas, et seuls les voxels **devant** comptent :
  on ne peut plus rester coincé ni être éjecté. Les gravats sont de vrais obstacles
  (on monte dessus), les marches se gravissent et on reste collé au sol en descente.
- **Cubes de gravats** : leur logique était inversée. Casser un voxel faisait apparaître
  un bloc presque plein **dans le mur restant**, qui disparaissait ensuite quand on
  finissait de casser la zone. Maintenant, chaque bloc a la taille exacte des voxels
  cassés et apparaît dans le trou. Il ne tourne plus que sur l'axe vertical, donc sa
  boîte de collision correspond à ce qu'on voit (fini les cubes penchés qui flottent),
  et il retombe si son support est détruit. Le nombre de blocs est limité.
- **Rendu** : une grosse destruction ne mettait à jour que 8 morceaux de la carte, ce
  qui laissait des voxels « fantômes » visibles. Le reste était perdu, et `F5` ne
  reconstruisait presque rien. Les faces au bord de la carte manquaient aussi, et les
  gravats n'avaient pas la même couleur que les murs (conversion sRGB/linéaire).
- **Armes** : la caméra des armes repeignait l'écran (couleur du ciel), la C4 posée
  mesurait 4 cm, la traçante était mal orientée, et la secousse laissait la caméra
  penchée. Toutes les armes ont désormais leur modèle et leurs animations (coup,
  recul, recharge, lancer, sortie d'arme).
- **Sons** : chaque son créait une entité jamais supprimée (fuite de mémoire). Les sons
  sont maintenant synthétisés une seule fois au démarrage.
- **Réseau** : le serveur rejetait tout tir à plus de 8 m (Glock compris), refusait la
  première position du joueur, détruisait des voxels sans les diffuser quand le budget
  était dépassé, et la C4 était dupliquée chez celui qui la posait. Les trames pouvaient
  aussi être coupées sur un socket non bloquant. Le serveur dédié tournait à 100 % d'un cœur.

## Développement

```bash
cargo run                              # le jeu (debug optimisé)
cargo run --bin boomground-server      # serveur dédié
cargo test                             # tests (collisions, maillage, protocole, boutique…)
```

Construire les installateurs :

```bash
powershell -ExecutionPolicy Bypass -File scripts/package.ps1   # Windows
sh scripts/package.sh                                          # Linux / macOS
```

Les fichiers sont générés dans `dist/`. Le workflow GitHub `.github/workflows/release.yml`
construit les trois systèmes à chaque push, et publie une release sur un tag `v*`.
Sous Linux, la compilation nécessite `libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`.

Mode test automatique : `BOOM_AUTO=1` (captures dans `<dossier temporaire>/boomground/`),
avec `BOOM_WEAPON=rifle`, `BOOM_MODE=normal`, `BOOM_SEED=42`, `BOOM_SHOP=1`,
`BOOM_MENU=1`, `BOOM_OVERVIEW=1`, `BOOM_SERVER=hôte`.

## Architecture

| Module | Rôle |
|---|---|
| `world.rs` | grille 384 × 64 × 384 (bitset + 1 octet de matériau), rayon DDA, `carve`, `sweep` sans traversée ni téléportation |
| `map.rs` | génération de la ville à partir de la seed |
| `vmesh.rs` | greedy meshing par morceaux de 16³ (opaque + vitres translucides), file de reconstruction |
| `rubble.rs` | blocs de gravats : chute, empilement, sommeil, réveil si le support disparaît |
| `debris.rs` | petits éclats décoratifs |
| `player.rs` | caméra FPS, collisions, marches, vol libre |
| `weapon.rs` | les 13 armes : modèles en main, actions, bombes, explosions, souffle |
| `game.rs` | modes, catalogue d'armes, inventaire, argent, réglages enregistrés |
| `shop.rs` | boutique |
| `menu.rs`, `hud.rs`, `ui.rs` | interface (police embarquée dans l'exécutable) |
| `fx.rs` | sons synthétisés, secousse, flashs, boules de feu |
| `net/` | protocole binaire, client, serveur (dédié ou hébergé dans le jeu) |
| `installer/` | installateur multiplateforme qui embarque le jeu |
