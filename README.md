# BoomGround

Bac à sable de destruction en voxels, façon *Teardown*, écrit en Rust avec Bevy.
**Tout est destructible** (sauf le socle) : immeubles, vitres, voitures, arbres, routes, sol.
Quand un pilier saute, ce qu'il portait s'écroule vraiment.

> Licence : source disponible, **tous droits réservés**. Tu peux jouer, lire le code et
> contribuer ; tu ne peux pas le copier ni le redistribuer. Voir [LICENSE](LICENSE).

## Télécharger et jouer

Les fichiers précompilés sont dans les [releases](https://github.com/Riric65/boomground/releases).

| Système | Jeu | Serveur dédié (facultatif) |
|---|---|---|
| Windows | `BoomGround-Setup.exe` | `BoomGround-server.exe` |
| Linux | `BoomGround-x86_64.AppImage` (`chmod +x`, puis double-clic) | `BoomGround-server-linux-x86_64` |

Le jeu est **un seul fichier**. Au premier lancement, il propose de s'installer (raccourcis,
menu des applications, commande `boomground`, pare-feu sous Windows) ou de jouer sans
installer. Sous Linux, l'AppImage s'installe elle-même dans `~/.local/share/boomground`.
Détails : [Installation](#installation).

## Ce que contient le jeu

- **Une grande ville** de 845 × 845 m (33 × 33 pâtés de maisons), générée depuis une *seed* :
  centre-ville de tours, quartiers d'immeubles, banlieue de maisons, entrepôts, parcs,
  parkings, chantiers.
- **Intérieurs meublés** : tables et chaises, bureaux, canapés, étagères, plafonniers,
  lampadaires d'intérieur.
- **Mobilier urbain** : lampadaires, bancs, bouches d'incendie, poubelles, feux tricolores.
- **Portes** qui s'ouvrent et se ferment (`F`), et que l'on peut aussi démolir.
- **Construction** : l'outil *Blocs* pose des blocs de bois, brique, béton, métal, verre,
  plâtre, tuile, noir ou lumière.
- **Physique** : chaque voxel doit être relié au sol par un chemin qui tient. Un pilier porte
  tout, mais à l'horizontale la portée dépend du matériau (béton ≈ 5 m, bois ≈ 5 m, brique
  ≈ 4 m, verre ≈ 2 m). Un plancher sans pilier, une façade au-dessus d'un grand trou, un
  porte-à-faux trop long : **tombent**. L'ensemble détaché devient un corps rigide (masse,
  centre de masse, inertie, 9,81 m/s²) qui bascule, heurte le monde, se casse à l'impact et
  se fige. Le mobilier ne fait pas partie du bâtiment : il tombe de son côté.
- **Feu** : le bois, le feuillage et le tissu brûlent. Les explosions les enflamment, le feu
  se propage (plus vite vers le haut), consume les voxels (ce qu'ils portaient peut tomber)
  et brûle les joueurs proches.
- **Poussière et fumée** : nuages à chaque écroulement, impact et explosion.
- **Son et musique** : sons normalisés et spatialisés à plus longue portée ; trois ambiances
  musicales composées par le jeu (aucun fichier, aucune licence). Dépose tes propres `.wav` ou
  `.ogg` dans le dossier `music` des réglages pour les utiliser à la place.
- **Mort** : 100 points de vie (explosions, balles, chutes, écrasement). À 0, le personnage
  devient un ragdoll, la caméra passe à la troisième personne, puis tu réapparais.
- **Mondes sauvegardables** : un fichier `.bgw` contient **tout** (ville modifiée, gravats,
  corps en chute, portes, joueurs, argent, munitions). Menu → *Sauvegarder le monde* ;
  onglet Jouer → *Mondes sauvegardés*. Les serveurs sauvegardent aussi automatiquement.
- **Une seule logique de jeu** : le solo lance un serveur privé local. Tout ce qui existe
  en solo (physique, dégâts, argent, portes, blocs, sauvegardes) existe donc en ligne.
- **76 outils** rangés en 6 catégories (touches `1`–`6`) : mêlée, pistolets, fusils, armes
  lourdes, explosifs, outils. Lance-flammes, extincteur, mines, napalm, canon, Tsar…
- **Véhicules** : monte dans une voiture garée avec `F` et conduis-la (`ZQSD`, `Espace`
  frein, `H` klaxon). Le moteur est un vrai corps rigide : une voiture trop abîmée ne roule plus.
- **Multijoueur** : liste de serveurs enregistrés (comme Minecraft), lobbys sur le réseau
  local, code via Internet (UPnP), serveur dédié, **chat** (`T`) modéré et **voix de proximité**
  (suppression de bruit, limiteur).
- **Discord Rich Presence** : solo (mode, voxels détruits) ou serveur (nom, joueurs).
- **Chargement complet** : la ville est générée et maillée avant de jouer.

## Commandes

| Touche | Action |
|---|---|
| ZQSD / WASD, flèches | se déplacer |
| Maj | courir |
| Espace | sauter (en vol libre : monter ; Ctrl/C : descendre) |
| Souris | regarder (clic = capturer le curseur) |
| Clic gauche | agir avec l'outil en main |
| Clic droit | faire sauter les C4 · changer de matériau (outil *Blocs*) |
| `F` | ouvrir / fermer une porte · monter / descendre d'une voiture |
| `T` / Entrée | écrire dans le chat (en ligne) |
| `G` (maintenu) | parler, si la voix est réglée sur « touche » |
| `H` | klaxon (en voiture) |
| 1…6 | catégorie d'armes (encore : arme suivante de la catégorie) |
| molette, Q / E | arme précédente / suivante |
| R | recharger |
| B | boutique (mode normal) |
| Tab (maintenu) | liste des joueurs et code de la partie (en ligne) |
| V | vol libre (bac à sable) |
| F5 / F6 | même ville / nouvelle ville (solo) |
| Échap | menu (pause) |

Le HUD n'affiche plus d'aide permanente : tout est ici.

## Modes de jeu

- **Bac à sable** : tous les outils, munitions et bombes illimitées, vol libre, blocs gratuits.
- **Normal** : on commence avec un marteau. Chaque voxel détruit rapporte de l'argent
  (métal 3 $, brique/verre/tuile 2 $, béton/bois/plâtre 1 $, lumière 3 $ ; le sol ne
  rapporte rien). La boutique (`B`) vend les autres outils. Poser un bloc coûte 1 $.

## Arsenal

| Famille | Outils |
|---|---|
| Marteaux | Marteau, Masse, Masse de guerre, Pioche, Marteau-piqueur (continu) |
| Armes à feu | Glock 17, Revolver .44, Pistolet-mitrailleur, Fusil à pompe, Fusil d'assaut, Minigun, Sniper .50, Railgun, Lance-roquettes |
| Bombes lancées | Grenade (rebondit), Dynamite (colle à la surface) |
| Bombes posées | C4 (clic droit pour les faire sauter), Méga-bombe (minuterie de 8 s) |
| Construction | Blocs |

Les explosions s'enchaînent : une bombe prise dans un souffle explose à son tour.

## Carte et seed

- Dans le menu, la seed est un nombre ou un mot (`paris` donne toujours la même ville).
  « Hasard » en tire une nouvelle.
- La carte se charge progressivement autour de toi au démarrage (quelques secondes).

## Réglages

Copier / coller (`Ctrl+C`, `Ctrl+V`) fonctionne dans tous les champs.

Menu → Paramètres : sensibilité, champ de vision, secousses de caméra, inversion de l'axe Y,
balancement à la marche, volumes (général, bruitages, musique), **préréglage graphique**
(Bas, Moyen, Élevé, Ultra, Personnalisé) et ses réglages (distance d'affichage, ombres,
anticrénelage, particules, bloom), limite d'images, plein écran, compteur de FPS, viseur. Ils sont enregistrés dans
`%APPDATA%\BoomGround` (Windows) ou `~/.config/boomground` (Linux).

## Multijoueur

**Créer un lobby** (Multijoueur → Créer un lobby) lance le serveur en fond, dans le jeu.
Il annonce la partie sur le réseau local et demande à ta box d'ouvrir le port **28431**
(UPnP). Les joueurs d'Internet entrent avec le **code de la partie** (ex. `K7M2Q-9XH4C`).
L'hôte clique sur **Lancer la partie** ; un joueur qui arrive ensuite entre directement.

Le serveur est l'autorité : il simule la physique, tient l'argent et les munitions de chaque
joueur, valide chaque tir, pose et ouverture de porte, applique les dégâts, et contrôle les
déplacements (vitesse, traversée de murs, vol, chutes). Si des amis d'Internet n'entrent pas, le lobby affiche la raison (UPnP désactivé,
box derrière un CGNAT…) ; redirige alors le port 28431 (TCP) à la main, ou utilise un réseau
virtuel (Tailscale, ZeroTier).

### Serveur dédié

Lance `BoomGround-server` : au premier démarrage il crée un **`server.conf`** à côté de
l'exécutable (nom, port, seed, mode, joueurs max, pvp, dégâts, réapparition). Modifie-le
puis relance. Voir [SERVER.md](SERVER.md).

## Installation

Au premier lancement, le jeu affiche son écran d'installation :
- **Installer** : Windows → `%LOCALAPPDATA%\Programs\BoomGround`, raccourcis Bureau et menu
  Démarrer ; Linux → `~/.local/share/boomground`, entrée du menu des applications, commande
  `boomground`. Sans droits administrateur. Une case ajoute la règle de pare-feu (Windows).
- **Jouer sans installer** : version portable (`--portable`).

Désinstaller : « Applications installées » → BoomGround, ou Paramètres → Désinstaller.

## Compiler

```bash
cargo run --release                 # le jeu
cargo run --release --bin boomground-server   # serveur dédié
cargo test --release                # tests
```

Paquets distribuables :

```bash
powershell -ExecutionPolicy Bypass -File scripts/package.ps1   # Windows → dist/
sh scripts/package-appimage.sh                                 # Linux → dist/
```

Sous Linux : `pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`.
Le workflow `.github/workflows/release.yml` compile Windows et Linux à chaque push et publie
une release sur un tag `v*`.

Variables de test : `BOOM_AUTO=1` (le jeu joue seul), `BOOM_WEAPON`, `BOOM_MODE=normal`,
`BOOM_SEED`, `BOOM_SHOP=1`, `BOOM_MENU=1`, `BOOM_HOST=1`, `BOOM_JOIN=code`.

## Architecture

| Module | Rôle |
|---|---|
| `world.rs` | grille en morceaux 16³ alloués à la demande, rayon DDA, `carve`, détection des ensembles flottants, `sweep`, portes |
| `sim.rs` | physique du serveur : gravats, corps rigides (collisions, impacts, scission, figeage), sérialisation |
| `map.rs` | génération de la ville : quartiers, bâtiments, mobilier, mobilier urbain |
| `vmesh.rs` | greedy meshing par morceaux (opaque, vitres, lampes) avec file de reconstruction |
| `rubble.rs` | gravats rigides avec masse, empilement, sommeil |
| `vitals.rs` | mort, ragdoll à particules, caméra de mort, réapparition (côté client) |
| `weapon.rs`, `game.rs` | outils, construction, interactions ; état, réglages, spécifications |
| `net/` | protocole, client + reflet de la simulation, serveur (autorité), `server.conf`, sauvegardes `.bgw`, journal, réseau local, UPnP |
| `menu.rs`, `hud.rs`, `shop.rs`, `ui.rs` | interface |
| `install.rs` | installation autonome |

## Contribuer

Les contributions sont bienvenues : voir [CONTRIBUTING.md](CONTRIBUTING.md). L'historique des
changements est dans [CHANGELOG.md](CHANGELOG.md).
