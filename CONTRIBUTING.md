# Contribuer à BoomGround

Merci de vouloir aider ! Les contributions sont bienvenues, dans le cadre de la
[licence](LICENSE).

## Ce que tu peux faire

- Forker le dépôt sur GitHub et proposer une *pull request*.
- Signaler un bug ou proposer une idée via une *issue*.

En contribuant, tu acceptes que ta contribution soit placée sous la licence du projet et que
le propriétaire puisse l'utiliser, la modifier et la redistribuer (article 4 de la licence).
Ne soumets que du code dont tu es l'auteur.

## Avant de proposer une modification

```bash
cargo fmt
cargo test --release
cargo clippy --release
```

- Le jeu et le serveur partagent le même monde : toute modification du protocole
  (`src/net/protocol.rs`) doit incrémenter `PROTOCOL_VERSION`.
- Le monde est **déterministe** à partir de la seed : n'utilise pas d'aléa non seedé dans
  `map.rs`.
- Garde les commentaires en français, comme le reste du code, et reste sobre.

## Hors périmètre

Copier le code dans un autre projet, redistribuer des binaires ou publier un fork
distribuable n'est pas permis sans autorisation écrite.
