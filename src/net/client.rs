//! Client réseau : connexion TCP, envoi d'état, application des décisions du
//! serveur. Aucune dépendance externe — on lit/écrit dans le socket.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use bevy::prelude::*;

use crate::game::{GameState, Weapon};
use crate::net::protocol::{read_into, split_address, write_packet, FrameReader, HitKind, Packet};
use crate::rubble::{self, Chunks};
use crate::world::{VoxelAssets, VoxelWorld};

/// Représentation d'un joueur distant (pour l'afficher).
#[derive(Component)]
pub struct RemotePlayer {
    pub id: u16,
    pub name: String,
}

#[derive(Resource)]
pub struct NetClient {
    /// Adresse saisie (`hôte` ou `hôte:port`).
    pub address: String,
    stream: Option<TcpStream>,
    reader: FrameReader,
    outbox: Vec<Packet>,
    pub id: Option<u16>,
    pub connected: bool,
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
            connected: false,
            status: String::new(),
        }
    }
}

impl NetClient {
    /// Tente de se connecter. Retourne l'adresse réellement visée.
    pub fn connect(&mut self) -> Result<String, String> {
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
            match TcpStream::connect_timeout(socket, Duration::from_secs(5)) {
                Ok(stream) => {
                    stream.set_nonblocking(true).ok();
                    let mut clone = stream.try_clone().map_err(|e| e.to_string())?;
                    write_packet(
                        &mut clone,
                        &Packet::Join {
                            name: "joueur".into(),
                        },
                    )
                    .map_err(|e| e.to_string())?;
                    self.stream = Some(stream);
                    self.connected = true;
                    self.status = format!("connecté à {socket}");
                    return Ok(socket.to_string());
                }
                Err(e) => last_error = Some(e),
            }
        }

        Err(match last_error {
            Some(ref e) if e.kind() == std::io::ErrorKind::ConnectionRefused => format!(
                "aucun serveur sur {target} — sur la machine qui héberge : \
                 `cargo run --bin boomground-server`"
            ),
            Some(e) => format!("{target} : {e}"),
            None => format!("{target} : aucune adresse joignable"),
        })
    }

    pub fn is_online(&self) -> bool {
        self.stream.is_some()
    }

    /// Envoie un paquet quelconque (utilisé par les armes).
    pub fn send_raw(&mut self, packet: Packet) {
        if !self.is_online() {
            return;
        }
        self.send(packet);
        self.flush();
    }

    fn send(&mut self, packet: Packet) {
        self.outbox.push(packet);
    }

    fn flush(&mut self) {
        let Some(stream) = self.stream.as_mut() else {
            return;
        };
        while let Some(packet) = self.outbox.first() {
            match write_packet(stream, packet) {
                Ok(()) => {
                    self.outbox.remove(0);
                }
                Err(e) => {
                    self.status = format!("réseau : {e}");
                    self.stream = None;
                    self.connected = false;
                    return;
                }
            }
        }
    }

    fn drain(&mut self) -> Vec<Packet> {
        let Some(stream) = self.stream.as_mut() else {
            return Vec::new();
        };
        match read_into(stream, &mut self.reader) {
            Ok(0) => {
                self.stream = None;
                self.connected = false;
                self.status = "déconnecté".into();
            }
            Ok(_) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => {
                self.status = format!("réseau : {e}");
                self.stream = None;
                self.connected = false;
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

/// Envoie la position et le regard du joueur local au serveur.
pub fn send_state(
    mut net: ResMut<NetClient>,
    game: Res<GameState>,
    players: Query<(&Transform, &crate::player::Body), With<crate::player::Player>>,
) {
    if !net.is_online() {
        return;
    }
    let Ok((transform, body)) = players.single() else {
        return;
    };
    let weapon = match game.weapon {
        Weapon::Sledge => 0,
        Weapon::Glock => 1,
        Weapon::C4 => 2,
    };
    net.send(Packet::State {
        pos: transform.translation,
        yaw: body.yaw,
        pitch: body.pitch,
        weapon,
    });
    net.flush();
}

/// Informe le serveur d'une action d'arme (déjà prédite en local).
pub fn send_hit(
    net: &mut NetClient,
    kind: HitKind,
    origin: Vec3,
    dir: Vec3,
    blast: f32,
) {
    if !net.is_online() {
        return;
    }
    net.send(Packet::Hit {
        kind,
        origin,
        dir,
        blast,
    });
    net.flush();
}

/// Applique ce que le serveur a décidé.
#[allow(clippy::too_many_arguments)]
pub fn receive(
    mut commands: Commands,
    mut net: ResMut<NetClient>,
    mut world: ResMut<VoxelWorld>,
    mut chunks: ResMut<Chunks>,
    assets: Res<VoxelAssets>,
    c4_assets: Res<crate::weapon::C4Assets>,
    mut rng: ResMut<crate::debris::Rng>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut remotes: Query<(Entity, &RemotePlayer, &mut Transform), Without<crate::player::Player>>,
) {
    if !net.is_online() {
        return;
    }

    // Un seul appel système par trame reçue.
    for packet in net.drain() {
        match packet {
            Packet::Welcome { id } => {
                net.id = Some(id);
            }
            Packet::Voxels { cells } => {
                // Autorité du serveur : on applique (idempotent).
                for c in cells {
                    let cell = IVec3::new(c[0] as i32, c[1] as i32, c[2] as i32);
                    if world.is_solid(cell) {
                        let center = world.cell_center(cell);
                        let destroyed = world.carve(center, 0.2);
                        for d in destroyed {
                            let at = world.cell_center(d);
                            crate::rubble::break_voxel(
                                &mut chunks, &mut commands, &assets, &world, d, at, &mut rng,
                            );
                        }
                    }
                }
            }
            Packet::Player {
                id,
                pos,
                yaw,
                pitch,
                ..
            } => {
                let found = remotes
                    .iter()
                    .find(|(_, r, _)| r.id == id)
                    .map(|(e, _, _)| e);
                match found {
                    Some(entity) => {
                        if let Ok((_, _, mut t)) = remotes.get_mut(entity) {
                            t.translation = pos + Vec3::Y * 0.85;
                            t.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
                        }
                    }
                    None => {
                        let mesh = meshes.add(Capsule3d::new(0.32, 1.1));
                        let mat = materials.add(StandardMaterial {
                            base_color: Color::srgb(0.25, 0.55, 0.95),
                            perceptual_roughness: 0.8,
                            ..default()
                        });
                        commands.spawn((
                            RemotePlayer { id, name: format!("joueur {id}") },
                            Mesh3d(mesh),
                            MeshMaterial3d(mat),
                            Transform::from_translation(pos + Vec3::Y * 0.85)
                                .with_rotation(Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0)),
                        ));
                    }
                }
            }
            Packet::PlayerLeft { id } => {
                for (entity, remote, _) in &remotes {
                    if remote.id == id {
                        commands.entity(entity).despawn();
                    }
                }
            }
            Packet::C4Placed { at } => {
                crate::weapon::place_c4(&mut commands, at, &c4_assets);
            }
            Packet::Explode { at, radius } => {
                crate::weapon::detonate(at, &mut world, &mut chunks, &mut commands, &assets, &mut rng);
                let _ = radius;
            }
            Packet::Chunks { spawned, removed } => {
                for c in spawned {
                    rubble::spawn_from_net(
                        &mut chunks,
                        &mut commands,
                        &assets,
                        IVec3::new(c.key[0] as i32, c.key[1] as i32, c.key[2] as i32),
                        Vec3::new(c.at[0], c.at[1], c.at[2]),
                        c.material,
                    );
                }
                for key in removed {
                    let key = IVec3::new(key[0] as i32, key[1] as i32, key[2] as i32);
                    if let Some(state) = chunks.map.remove(&key) {
                        commands.entity(state.entity).despawn();
                    }
                }
            }
            // Paquets destinés au serveur : ignorés côté client.
            Packet::Join { .. } | Packet::State { .. } | Packet::Hit { .. } => {}
        }
    }
}