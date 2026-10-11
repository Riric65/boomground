# BoomGround

Destruction de voxels à la masse, au Glock et à la C4 — en Rust + Bevy, style
Teardown. **Tout est destructible : bâtiments, murs et sol.**

## Lancer

```bash
cargo run                      # le client
cargo run --bin boomground-server   # le serveur, port 28431
```

| Variable | Effet |
|---|---|
| `BOOM_FPS=144` | force la limite d'images (sinon : celle de l'écran) |
| `BOOM_PORT=3000` | port du serveur (défaut `28431`) |
| `BOOM_AUTO=1` | autopilot de test + captures dans `/tmp/opencode/boom/` |
| `BOOM_WEAPON=glock\|c4\|sledge` | force une arme en mode auto |
| `BOOM_NO_DRIFT=1` / `BOOM_OVERVIEW=1` / `BOOM_MENU=1` / `BOOM_SETTINGS=1` | fige la caméra / vue du dessus / reste au menu / ouvre les réglages |

## Contrôles

| Touche | Action |
|---|---|
| ZQSD / WASD | se déplacer (balancement de marche, atterrissage) |
| Souris | regarder (clic = capturer le curseur) |
| Clic gauche | agir avec l'arme en main |
| 1 / 2 / 3 ou molette | masse / Glock 17 / C4 |
| R | recharger le Glock |
| F5 | reconstruire le niveau |
| Échap / F1 | menu |

Menu **PARAMÈTRES** : sensibilité, volume, limite d'images, inversion de l'axe Y.

## Multijoueur

Sur la machine qui héberge : `cargo run --bin boomground-server` (écoute sur
`0.0.0.0:28431`). Les clients choisissent **MULTIJOUEUR** et saisissent
`domaine.tld`, `ip`, ou `ip:port` (sinon port **28431** par défaut).
Si aucun serveur ne répond, le client le dit explicitement au lieu de renvoyer
une erreur brute.

- **Prédiction locale** puis **autorité du serveur** sur les voxels et les
  gravats : réponse immédiate chez soi, état juste pour tout le monde.
- **Anti-triche** : cadence par arme, distance du rayon par rapport à la
  position connue, rayon borné, cohérence du tir, budget de destruction
  (300 voxels/s/joueur), téléportation bornée à 20 m/s, C4 à portée.
- **Le son est en 3D** (`SpatialListener` sur la caméra) : un coup de feu
  s'entend du bon côté et s'atténue avec la distance ; le tic de la C4 vient
  de la C4.

Vérifié par un client de test : flood de 50 balles → 48 rejets, tir à 500 m →
rejeté, tirs à cadence normale → voxels + gravats diffusés.

## Physique

| Corps | Collisions |
|---|---|
| Joueur | voxels (boîte 0,6 × 1,7 × 0,6 m) **+ blocs de gravats** : on gravit un tas au lieu de le traverser |
| Blocs de gravats | voxels du terrain + blocs voisins déjà posés (grille spatiale), puis sommeil définitif |
| Débris | voxels du terrain, rebond amorti, frottement au sol : ils ricochent puis se posent |

Il n'existe **aucun plan de sol imaginaire** : tout repose sur la grille de
voxels, donc un trou creusé dans le sol laisse tomber ce qui se trouve
dessus.

## Réseau

`localhost` se résout d'abord en `::1` alors que le serveur n'écoute qu'en
IPv4 : le client essaie donc **toutes les adresses résolues** avant d'abandonner,
sinon le multijoueur local échoue systématiquement.

## Architecture

| Module | Rôle |
|---|---|
| `world.rs` | grille de 128×56×128 : **bitset** d'occupation + 1 octet de matériau par cellule (~1 Mo pour 105 000 voxels), rayon DDA, `carve` sans entité, `sweep` partagé par toutes les collisions |
| `map.rs` | génération de la carte : sol, route, trottoirs, 4 bâtiments (fenêtres, portes, étages, murs de 2 voxels d'épaisseur), caisses |
| `vmesh.rs` | **greedy meshing** par morceaux de 8³ : ~650 maillages fusionnés, couleurs aux sommets ; seuls les morceaux touchés sont reconstruits (8 par frame max) |
| `rubble.rs` | blocs de gravats 2×2×2 persistants : gravité, rebonds, collisions bloc/bloc (grille spatiale), sommeil ; un bloc rétrécit à chaque voxel cassé |
| `sledge.rs` | masse à trois temps (manche en bas à droite, tête au centre), son, secousse, gravats |
| `weapon.rs` | Glock 17 (traçantes, recul, éclat de bouche) et C4 |
| `fx.rs` | sons **synthétisés** (WAV encodé à la volée, aucun fichier), secousse de caméra, flashs |
| `player.rs` | caméra FPS, collisions voxel + gravats, balancement de marche |
| `perf.rs` | `AutoVsync` + boucle bornée à la fréquence de l'écran |
| `net/` | protocole binaire maison, client, serveur anti-triche |

## Performance

Le passage d'« une entité par voxel » à « un maillage fusionné par morceau »
a fait passer ~3 000 entités / ~3 000 draw calls à **650 maillages** avec la
carte complète (105 714 voxels) : **239–247 FPS** sur une RTX 3060 en 1600×900,
sol compris, avec des centaines de blocs de gravats en mouvement.

## Tests

`cargo test` vérifie l'encodage WAV des bruitages et le maillage fusionné
(fusion de faces, plan de la face supérieure, morceau vide).