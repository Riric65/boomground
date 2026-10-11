# BoomGround

Destruction de voxels façon Teardown, en Rust + Bevy. **Tout est destructible**
(sauf la couche de socle tout en bas) : immeubles, vitres, voitures, arbres, routes, sol.

## Installer (joueurs)

**Un seul fichier**, rien à compiler, aucun terminal :

| Système | Fichier |
|---|---|
| Windows | `BoomGround-Setup.exe` |
| Linux | `BoomGround-x86_64.AppImage` (`chmod +x` puis double-clic) |

Au premier lancement, le jeu affiche son propre écran d'installation :
- **Installer** : le jeu se copie dans le dossier des programmes de l'utilisateur, sans
  droits administrateur (`%LOCALAPPDATA%\Programs\BoomGround` sous Windows,
  `~/.local/share/boomground` sous Linux, `~/Applications/BoomGround.app` sous macOS).
  Il crée aussi un raccourci sur le Bureau et dans le menu Démarrer ou le menu des
  applications, puis il se relance.
- Sous Windows, une case « Autoriser le multijoueur dans le pare-feu » ajoute la règle
  qui laisse entrer tes amis. Windows demande une confirmation, une seule fois.
- **Jouer sans installer** : le jeu se lance tel quel (version portable).

Relancer un fichier plus récent propose **Mettre à jour**. Pour désinstaller : Windows →
« Applications installées » → BoomGround, ou Paramètres → Désinstaller dans le jeu.

Sur macOS, l'application n'est pas signée : la première fois, fais clic droit puis **Ouvrir**.

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
| B | boutique (mode normal) |
| Tab (maintenu) | joueurs et code de la partie (en ligne) |
| V | vol libre (bac à sable) |
| H | masquer l'aide |
| F5 / F6 | même ville / nouvelle ville |
| Échap | menu (pause) |

Les réglages (pseudo, sensibilité, volume, champ de vision, limite d'images, axe Y,
plein écran) sont enregistrés
dans `%APPDATA%\BoomGround` (Windows), `~/Library/Application Support/BoomGround`
(macOS) ou `~/.config/boomground` (Linux).

## Multijoueur

**Créer un lobby** (Multijoueur → Créer un lobby) lance le serveur de la partie **en fond,
dans le jeu lui-même**, sans fenêtre ni terminal. Ce serveur :
- annonce la partie sur le réseau local : tes amis sur le même Wi-Fi la voient
  apparaître toute seule dans « Parties sur ton réseau », il suffit de cliquer ;
- demande à ta box d'ouvrir le port **28431** (UPnP), pour que les joueurs d'Internet
  entrent avec le **code de la partie** (ex. `K7M2Q-9XH4C`, bouton « Copier »). La box
  referme le port quand le lobby se ferme.

Dans le lobby, l'hôte voit la liste des joueurs et clique sur **Lancer la partie**. Un joueur
qui arrive ensuite entre directement, avec la ville dans son état actuel.

Pour **rejoindre** : clique sur une partie de la liste, ou saisis le code (ou une adresse IP)
puis « Rejoindre ». Les joueurs se voient (avatar coloré et pseudo au-dessus de la tête), et
chacun voit ce que les autres cassent. Le serveur valide chaque action (cadence, portée,
rayon maximal par arme, téléportation). Le mode de la partie (bac à sable ou normal)
est choisi par l'hôte.

**Si les amis d'Internet n'arrivent pas à entrer**, le lobby affiche la raison. En général,
la box refuse l'ouverture automatique : active l'UPnP dans l'interface de la box, ou
redirige à la main le port 28431 (TCP) vers le PC de l'hôte. Si la box est derrière le
réseau de l'opérateur (CGNAT), seule une redirection côté opérateur ou un réseau virtuel
(Tailscale, ZeroTier…) permet de jouer depuis Internet.

Serveur dédié (facultatif) : `BoomGround-Setup.exe --server` (variables `BOOM_PORT`,
`BOOM_SEED`, `BOOM_MODE=normal`, `BOOM_NAME`).

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
cargo run                       # le jeu (debug optimisé)
cargo run -- --server           # serveur dédié
cargo test                      # tests (collisions, maillage, protocole, codes de partie, réseau local…)
```

Construire le fichier à distribuer :

```bash
powershell -ExecutionPolicy Bypass -File scripts/package.ps1   # Windows → dist/BoomGround-Setup.exe
sh scripts/package-appimage.sh                                 # Linux → dist/BoomGround-x86_64.AppImage
```

Le workflow GitHub `.github/workflows/release.yml` construit les trois systèmes à chaque
push, et publie une release sur un tag `v*`. Sous Linux, la compilation nécessite
`libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev`.

Mode test automatique : `BOOM_AUTO=1` (captures dans `<dossier temporaire>/boomground/`),
avec `BOOM_WEAPON=rifle`, `BOOM_MODE=normal`, `BOOM_SEED=42`, `BOOM_SHOP=1`, `BOOM_MENU=1`
(`BOOM_TAB=multi`), `BOOM_HOST=1` (`BOOM_START_AFTER=10`), `BOOM_JOIN=code`,
`BOOM_QUIT_AFTER=20`, `BOOM_INSTALLER=1`.

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
| `menu.rs`, `hud.rs`, `ui.rs` | interface : menu, lobby, écran d'installation, HUD, champs de saisie, notifications (police embarquée) |
| `install.rs` | auto-installation (raccourcis, pare-feu, désinstallation) |
| `fx.rs` | sons synthétisés, secousse, flashs, boules de feu |
| `net/` | protocole et codes de partie, client, serveur de lobby (en fond ou dédié), découverte réseau local (`lan`), ouverture du port de la box (`upnp`) |

## Licence

Tous droits réservés : voir [LICENSE](LICENSE). Copie, modification et redistribution interdites sans autorisation.
