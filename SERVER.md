# Serveur dédié BoomGround

Un serveur sans fenêtre pour héberger une partie en continu.

## Lancer

| Système | Fichier | Lancement |
|---|---|---|
| Windows | `BoomGround-server.exe` | double-clic, ou depuis un terminal |
| Linux | `BoomGround-server-linux-x86_64` | `chmod +x` puis `./BoomGround-server-linux-x86_64` |

Au premier démarrage, un fichier **`server.conf`** est créé à côté de l'exécutable. Modifie-le
puis relance le serveur. Supprime-le pour le régénérer.

Sous Linux, le binaire a besoin de `libasound.so.2` (paquet `libasound2` sur Debian/Ubuntu),
même sans audio.

## server.conf

Créé à côté de l'exécutable au premier lancement, **commenté clé par clé**. Voici les clés :

| Clé | Défaut | Rôle |
|---|---|---|
| `name` | `Serveur BoomGround` | nom affiché dans la liste du réseau local |
| `port` | `28431` | port TCP d'écoute |
| `max_players` | `16` | de 1 à 16 |
| `max_per_ip` | `4` | connexions simultanées par adresse IP |
| `idle_kick_minutes` | `0` | expulse les inactifs (0 = jamais) |
| `log_level` | `info` | `debug`, `info`, `warn`, `error` |
| `world` | `world` | nom du monde : `worlds/<nom>.bgw` est chargé s'il existe |
| `seed` | `random` | graine d'un NOUVEAU monde : nombre, mot, ou `random` |
| `mode` | `sandbox` | `sandbox` ou `normal` |
| `autosave_minutes` | `5` | sauvegarde automatique (0 = jamais) |
| `pvp` | `true` | les joueurs peuvent se blesser |
| `damage_scale` | `1.0` | multiplicateur de dégâts (`0` = invulnérable) |
| `fall_damage` | `true` | dégâts de chute |
| `respawn_seconds` | `5` | délai avant réapparition |
| `spawn_protection` | `2` | secondes d'invulnérabilité après réapparition |
| `keep_inventory` | `true` | garde l'inventaire après une mort |
| `gravity_collapse` | `true` | les bâtiments sans base s'écroulent (corps rigides) |
| `allow_building` | `true` | autorise l'outil Blocs |
| `allow_doors` | `true` | portes interactives |
| `disabled_weapons` | *(vide)* | armes interdites, ex. `railgun,minigun,megabomb` |
| `starting_money` | `0` | argent de départ (mode normal) |
| `explosion_scale` | `1.0` | multiplicateur du rayon maximal des explosions |
| `anticheat` | `1` | `0` aucun, `1` normal, `2` strict |

Armes : `hammer, sledge, warhammer, pickaxe, jackhammer, glock, revolver, smg, shotgun,
rifle, minigun, sniper, railgun, rocket, grenade, dynamite, c4, megabomb, blocks`.

Les variables d'environnement `BOOM_PORT`, `BOOM_SEED`, `BOOM_MODE` (`normal`) et `BOOM_NAME`
priment sur le fichier.

## Monde, sauvegardes et journal

- **Monde** : `worlds/<world>.bgw`. Il contient la ville modifiée, les gravats, les corps en
  chute, les portes et les joueurs (position, santé, argent, munitions). Il est sauvegardé
  automatiquement, par la commande `save`, et à l'arrêt (`stop`).
- **Journal** : `logs/latest.log` (l'ancien est archivé en `logs/AAAA-MM-JJ-N.log`), dans le
  style Minecraft :
  ```
  [14:03:22] [Server thread/INFO]: Alex a rejoint la partie (#1, 192.168.1.20)
  [14:05:10] [Server thread/INFO]: Sam a été tué par Alex (balle)
  [14:06:00] [Server thread/WARN]: Alex expulsé : déplacement trop rapide
  ```
  L'heure est en UTC.
- **Bannis** : `banned-players.txt` (un pseudo par ligne).

## Commandes de la console

`help`, `list`, `say <texte>`, `kick <joueur>`, `ban <joueur>`, `pardon <joueur>`,
`save [nom]`, `reset [seed]`, `seed`, `stop`.

## Anti-triche

Le serveur est l'autorité : argent, munitions et armes possédées sont tenus côté serveur ;
chaque tir doit respecter la cadence, le chargeur et la portée ; chaque explosion doit
correspondre à une bombe armée ; les déplacements sont bornés en vitesse, vérifiés contre les
murs et contre le vol (hors bac à sable). Un joueur suspect est ramené à sa dernière position
valide, puis expulsé si cela continue.

## Joindre

Dans le jeu : Multijoueur → saisir `adresse:port` (ou l'adresse IP), puis Rejoindre. Sur un
serveur distant, ouvre le port TCP choisi (28431 par défaut) sur la box et le pare-feu.
Les clients et le serveur doivent avoir la même version du jeu.
