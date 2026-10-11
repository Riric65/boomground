//! Client réseau : connexion TCP, envoi d'état, application des décisions du
//! serveur. Aucune dépendance externe — on lit/écrit dans le socket.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use bevy::prelude::*;

use crate::game::{GameState, WeaponId};
use crate::net::protocol::{read_into, split_address, write_packet, FrameReader, HitKind, Packet};
use crate::weapon::{BombAssets, Destroyer};

/// Représentation d'un joueur distant (pour l'afficher).
#[derive(Component)]
pub struct RemotePlayer {
    pub id: u16,
}

#[derive(Resource)]
pub struct NetClient {
    /// Adresse saisie (`hôte` ou `hôte:port`).
    pub address: String,
    stream: Option<TcpStream>,
    reader: FrameReader,
    /// Octets en attente d'envoi (le socket est non bloquant : une trame
    /// peut partir en plusieurs fois sans être coupée).
    outbox: Vec<u8>,
    pub id: Option<u16>,
    pub status: String,
}

impl Default for NetClient {
    fn default() -> Self {
        Self {
            // Par défaut on vise une partie locale ; l'utilisateur peut
            // remplacer par un domaine ou une IP.
            address: "localhost".to_string(),
            stream: None,
            reader: FrameReader::default(),
            outbox: Vec::new(),
            id: None,
            status: String::new(),
        }
    }
}

impl NetClient {
    /// Tente de se connecter. Retourne l'adresse réellement visée.
    pub fn connect(&mut self) -> Result<String, String> {
        self.disconnect();
        let (host, port) = split_address(&self.address);
        if host.is_empty() {
            return Err("adresse vide : saisissez un domaine ou une IP".into());
        }
        let target = format!("{host}:{port}");
        let addrs: Vec<_> = target
            .to_socket_addrs()
            .map_err(|e| format!("{target} : {e}"))?
            .collect();
        if addrs.is_empty() {
            return Err(format!("{target} : hôte introuvable"));
        }

        // `localhost` se résout d'abord en IPv6 (::1) alors que beaucoup de
        // serveurs n'écoutent qu'en IPv4 : on essaie donc **toutes** les
        // adresses résolues avant d'abandonner.
        let mut last_error: Option<std::io::Error> = None;
        for socket in &addrs {
            match TcpStream::connect_timeout(socket, Duration::from_secs(3)) {
                Ok(stream) => {
                    stream.set_nodelay(true).ok();
                    let mut clone = stream.try_clone().map_err(|e| e.to_string())?;
                    write_packet(
                        &mut clone,
                        &Packet::Join {
                            name: "joueur".into(),
                        },
                    )
                    .map_err(|e| e.to_string())?;
                    stream.set_nonblocking(true).ok();
                    self.stream = Some(stream);
                    self.reader = FrameReader::default();
                    self.status = format!("connecté à {socket}");
                    return Ok(socket.to_string());
                }
                Err(e) => last_error = Some(e),
            }
        }

        Err(match last_error {
            Some(ref e) if e.kind() == std::io::ErrorKind::ConnectionRefused => format!(
                "aucun serveur sur {target} — sur la machine qui héberge, lancez \
                 « BoomGround Serveur »"
            ),
            Some(e) => format!("{target} : {e}"),
            None => format!("{target} : aucune adresse joignable"),
        })
    }

    /// Ferme la connexion (retour au solo).
    pub fn disconnect(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        self.outbox.clear();
        self.id = None;
    }

    pub fn is_online(&self) -> bool {
        self.stream.is_some()
    }

    /// Envoie un paquet quelconque (utilisé par les armes).
    pub fn send_raw(&mut self, packet: Packet) {
        if !self.is_online() {
            return;
        }
        self.outbox.extend_from_slice(&packet.encode());
        self.flush();
    }

    /// Envoie les octets en attente (appelé à chaque image).
    pub fn flush(&mut self) {
        let Some(stream) = self.stream.as_mut() else {
            return;
        };
        use std::io::Write;
        let mut sent = 0;
        while sent < self.outbox.len() {
            match stream.write(&self.outbox[sent..]) {
                Ok(0) => break,
                Ok(n) => sent += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    self.status = format!("réseau : {e}");
                    self.stream = None;
                    self.outbox.clear();
                    return;
                }
            }
        }
        self.outbox.drain(..sent);
    }

    fn drain(&mut self) -> Vec<Packet> {
        let Some(stream) = self.stream.as_mut() else {
            return Vec::new();
        };
        // On lit tout ce qui est disponible (l'historique d'une partie peut
        // faire plusieurs centaines de ko).
        loop {
            match read_into(stream, &mut self.reader) {
                Ok(0) => {
                    self.stream = None;
                    self.status = "déconnecté du serveur".into();
                    break;
                }
                Ok(_) => continue,
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    self.status = format!("réseau : {e}");
                    self.stream = None;
                    break;
                }
            }
        }

        let mut packets = Vec::new();
        while let Some(frame) = self.reader.next() {
            if let Some(packet) = Packet::decode(&frame) {
                packets.push(packet);
            }
        }
        packets
    }
}

/// Envoie la position et le regard du joueur local au serveur (20 fois/s).
pub fn send_state(
    time: Res<Time>,
    mut timer: Local<f32>,
    mut net: ResMut<NetClient>,
    game: Res<GameState>,
    players: Query<(&Transform, &crate::player::Body), With<crate::player::Player>>,
) {
    if !net.is_online() {
        return;
    }
    net.flush();
    *timer -= time.delta_secs();
    if *timer > 0.0 {
        return;
    }
    *timer = 0.05;
    let Ok((transform, body)) = players.single() else {
        return;
    };
    net.send_raw(Packet::State {
        pos: transform.translation,
        yaw: body.yaw,
        pitch: body.pitch,
        weapon: game.weapon.tag(),
    });
}

/// Informe le serveur d'une action d'arme (déjà prédite en local).
pub fn send_hit(net: &mut NetClient, kind: HitKind, origin: Vec3, dir: Vec3, blast: f32) {
    net.send_raw(Packet::Hit {
        kind,
        origin,
        dir,
        blast,
    });
}

/// Applique ce que le serveur a décidé.
#[allow(clippy::too_many_arguments)]
pub fn receive(
    mut d: Destroyer,
    bomb_assets: Res<BombAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut remotes: Query<(Entity, &RemotePlayer, &mut Transform)>,
    bombs: Query<(Entity, &crate::weapon::Bomb, &Transform), Without<RemotePlayer>>,
) {
    if !d.net.is_online() {
        // Partie quittée : les joueurs distants disparaissent.
        for (entity, _, _) in &remotes {
            d.commands.entity(entity).despawn();
        }
        return;
    }

    for packet in d.net.drain() {
        match packet {
            Packet::Welcome { id, seed } => {
                d.net.id = Some(id);
                // Tout le monde joue sur la carte du serveur.
                d.game.seed = seed;
                d.world.rebuild(seed);
            }
            Packet::Voxels { cells, rubble } => {
                // Autorité du serveur : on applique (idempotent : un voxel
                // déjà cassé en local est ignoré).
                let mut destroyed = Vec::new();
                for c in cells {
                    let cell = IVec3::new(c[0] as i32, c[1] as i32, c[2] as i32);
                    if d.world.is_solid(cell) {
                        d.world.set(cell, false, 0);
                        destroyed.push(cell);
                    }
                }
                if rubble && !destroyed.is_empty() {
                    let center = destroyed
                        .iter()
                        .map(|&c| d.world.cell_center(c))
                        .sum::<Vec3>()
                        / destroyed.len() as f32;
                    crate::rubble::spawn_rubble(
                        &mut d.rubble,
                        &mut d.commands,
                        &d.assets,
                        &d.world,
                        &destroyed,
                        center,
                        Vec3::ZERO,
                        &mut d.rng,
                    );
                }
            }
            Packet::Player {
                id, pos, yaw, pitch, ..
            } => {
                let rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch * 0.3, 0.0);
                let at = pos + Vec3::Y * 0.87;
                match remotes.iter_mut().find(|(_, r, _)| r.id == id) {
                    Some((_, _, mut t)) => {
                        t.translation = at;
                        t.rotation = rotation;
                    }
                    None => {
                        let mesh = meshes.add(Capsule3d::new(0.32, 1.1));
                        let mat = materials.add(StandardMaterial {
                            base_color: Color::srgb(0.25, 0.55, 0.95),
                            perceptual_roughness: 0.8,
                            ..default()
                        });
                        d.commands.spawn((
                            RemotePlayer { id },
                            Mesh3d(mesh),
                            MeshMaterial3d(mat),
                            Transform::from_translation(at).with_rotation(rotation),
                        ));
                    }
                }
            }
            Packet::PlayerLeft { id } => {
                for (entity, remote, _) in &remotes {
                    if remote.id == id {
                        d.commands.entity(entity).despawn();
                    }
                }
            }
            Packet::BombPlaced { at, weapon } => {
                let id = WeaponId::from_tag(weapon);
                crate::weapon::spawn_placed(&mut d.commands, &bomb_assets, id, at, Quat::IDENTITY, true, false);
            }
            Packet::Explode { at, radius } => {
                // La bombe distante disparaît, on voit et on entend
                // l'explosion ; les voxels arrivent par `Voxels`.
                for (entity, bomb, t) in &bombs {
                    if !bomb.local && t.translation.distance(at) < 1.5 {
                        d.commands.entity(entity).despawn();
                    }
                }
                d.effects(at, radius);
            }
            // Paquets destinés au serveur : ignorés côté client.
            Packet::Join { .. } | Packet::State { .. } | Packet::Hit { .. } => {}
        }
    }
}
