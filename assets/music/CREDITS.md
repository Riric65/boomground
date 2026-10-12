# Musique embarquée

Trois morceaux, tous en **CC0 1.0 Universal** (domaine public) : aucune
attribution n'est exigée, mais nous citons leurs auteurs par honnêteté.

| Fichier | Morceau | Auteur | Source |
| --- | --- | --- | --- |
| `menu.ogg` | *Exploring Town* | Spring Spring (multi-licence, dont CC0) | <https://opengameart.org/content/exploring-town> |
| `game.ogg` | *Summer Sunday* | celestialghost8 | <https://opengameart.org/content/summer-sunday> |
| `action.ogg` | *8-Bit Battle Loop* | Wolfgang_ | <https://opengameart.org/content/8-bit-battle-loop> |

Licence : <https://creativecommons.org/publicdomain/zero/1.0/>

## Traitement appliqué

Les fichiers d'origine ont été réencodés pour la distribution :

- **Ogg Vorbis, mono**, 44,1 kHz (le jeu n'a pas besoin de stéréo, et le
  décodeur Vorbis est déjà dans le moteur) ;
- coupes et fondus pour que les boucles ne claquent pas :
  - `menu.ogg` : 95 s conservés, fondu d'entrée 2 s / de sortie 7 s ;
  - `game.ogg` : 80 s conservés, fondu d'entrée 1,5 s / de sortie 6 s ;
  - `action.ogg` : la boucle complète (26,6 s), fondus courts ;
- volume global légèrement réduit (0,85 à 0,95) pour rester sous les
  bruitages.

Commande de réencodage (nécessite `ffmpeg`) :

```sh
ffmpeg -i menu.ogg  -t 95 -af "afade=t=in:st=0:d=2,afade=t=out:st=88:d=7,volume=0.85" -ac 1 -c:a libvorbis -b:a 96k menu.ogg
ffmpeg -i game.mp3  -t 80 -af "afade=t=in:st=0:d=1.5,afade=t=out:st=74:d=6,volume=0.9" -ac 1 -c:a libvorbis -b:a 112k game.ogg
ffmpeg -i action.ogg -t 40 -af "afade=t=in:st=0:d=1,afade=t=out:st=36:d=4,volume=0.95" -ac 1 -c:a libvorbis -b:a 112k action.ogg
```

## Ajouter vos propres morceaux

Déposez des `.ogg` ou des `.wav` dans le dossier `music` du répertoire de
configuration du jeu :

- Windows : `%APPDATA%\BoomGround\music`
- Linux : `~/.config/boomground/music`
- macOS : `~/Library/Application Support/BoomGround/music`

Ces morceaux **priment** sur les trois embarqués et sont joués en boucle.
