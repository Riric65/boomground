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

| Clé | Défaut | Rôle |
|---|---|---|
| `name` | `Serveur BoomGround` | nom affiché dans la liste du réseau local |
| `port` | `28431` | port TCP d'écoute |
| `seed` | `random` | nombre, mot, ou `random` (nouvelle ville à chaque démarrage) |
| `mode` | `sandbox` | `sandbox` ou `normal` |
| `max_players` | `16` | de 1 à 16 |
| `pvp` | `true` | les joueurs peuvent se blesser (`false` : seul l'auteur d'une explosion l'est) |
| `damage_scale` | `1.0` | multiplicateur de dégâts (`0` = invulnérable) |
| `respawn_seconds` | `5` | délai avant réapparition |

Les variables d'environnement `BOOM_PORT`, `BOOM_SEED`, `BOOM_MODE` (`normal`) et `BOOM_NAME`
priment sur le fichier.

## Joindre

Dans le jeu : Multijoueur → saisir `adresse:port` (ou l'adresse IP), puis Rejoindre. Sur un
serveur distant, ouvre le port TCP choisi (28431 par défaut) sur la box et le pare-feu.
Les clients et le serveur doivent avoir la même version du jeu.
