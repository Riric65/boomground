//! Client réseau : connexion au lobby, envoi d'état, application des
//! décisions du serveur, joueurs distants (avatar + nom au-dessus de la
//! tête). Aucune dépendance externe — on lit/écrit dans le socket.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use bevy::prelude::*;

use crate::game::{GameState, Mode, WeaponId};
use crate::net::protocol::{read_into, split_address, FrameReader, HitKind, Packet, PROTOCOL_VERSION};
use crate::net::server::HostHandle;
use crate::weapon::{BombAssets, Destroyer};

/// Représentation d'un joueur distant.
#[derive(Component)]
pub struct RemotePlayer {
    pub id: u16,
}

/// Étiquette de nom (interface) qui suit un joueur distant.
#[derive(Component)]
pub struct NameTag {
    pub id: u16,
}

/// Connexion au serveur.
#[derive(Resource, Default)]
pub struct NetClient {
    stream: Option<TcpStream>,
    reader: FrameReader,
    /// Octets en attente d'envoi (le socket est non bloquant : une trame
    /// peut partir en plusieurs fois sans être coupée).
    outbox: Vec<u8>,
    pub id: Option<u16>,
    /// La connexion a été perdue (pas un départ volontaire) : raison.
    pub lost: Option<String>,
}

/// La partie en ligne vue par ce joueur : lobby, joueurs, hôte.
#[derive(Resource, Default)]
pub struct Lobby {
    /// Dans un lobby ou une partie en ligne.
    pub active: bool,
    pub started: bool,
    pub mode: Mode,
    pub host_id: u16,
    pub players: Vec<(u16, String)>,
    /// Ce qu'on a saisi pour rejoindre (code ou adresse).
    pub joined_with: String,
    /// Partie hébergée par ce jeu (serveur en fond).
    pub hosting: Option<HostHandle>,
}

impl Lobby {
    pub fn is_host(&self) -> bool {
        self.hosting.is_some()
    }

    pub fn reset(&mut self) {
        *self = Lobby::default();
    }
}

impl NetClient {
    /// Se connecte et demande à entrer. Retourne l'adresse réellement visée.
    pub fn connect(&mut self, address: &str, name: &str, token: u64) -> Result<String, String> {
        self.disconnect();
        let (host, port) = split_address(address);
        if host.is_empty() {
            return Err("saisis un code de partie ou une adresse".into());
        }
        let target = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        let addrs: Vec<_> = target
            .to_socket_addrs()
            .map_err(|_| format!("« {host} » introuvable"))?
            .collect();
        if addrs.is_empty() {
            return Err(format!("« {host} » introuvable"));
        }

        // `localhost` se résout d'abord en IPv6 (::1) : on essaie **toutes**
        // les adresses résolues avant d'abandonner.
        let mut last_error: Option<std::io::Error> = None;
        for socket in &addrs {
            match TcpStream::connect_timeout(socket, Duration::from_secs(3)) {
                Ok(stream) => {
                    stream.set_nodelay(true).ok();
                    stream.set_nonblocking(true).ok();
                    self.stream = Some(stream);
                    self.reader = FrameReader::default();
                    self.lost = None;
                    self.send_raw(Packet::Join {
                        name: name.to_string(),
                        token,
                        version: PROTOCOL_VERSION,
                    });
                    return Ok(socket.to_string());
                }
                Err(e) => last_error = Some(e),
            }
        }

        Err(match last_error {
            Some(ref e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::TimedOut
                ) =>
            {
                "aucune partie à cette adresse (lobby fermé, ou box de l'hôte fermée)".into()
            }
            Some(e) => format!("connexion impossible : {e}"),
            None => "aucune adresse joignable".into(),
        })
    }

    /// Ferme la connexion volontairement.
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

    fn lose(&mut self, reason: impl Into<String>) {
        self.stream = None;
        self.outbox.clear();
        self.id = None;
        self.lost = Some(reason.into());
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
        use std::io::Write;
        let Some(stream) = self.stream.as_mut() else {
            return;
        };
        let mut sent = 0;
        let mut failed = None;
        while sent < self.outbox.len() {
            match stream.write(&self.outbox[sent..]) {
                Ok(0) => break,
                Ok(n) => sent += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        self.outbox.drain(..sent);
        if let Some(e) = failed {
            self.lose(format!("connexion perdue ({e})"));
        }
    }

    fn drain(&mut self) -> Vec<Packet> {
        let Some(stream) = self.stream.as_mut() else {
            return Vec::new();
        };
        // On lit tout ce qui est disponible (l'historique d'une partie peut
        // faire plusieurs centaines de ko).
        let mut closed = None;
        loop {
            match read_into(stream, &mut self.reader) {
                Ok(0) => {
                    closed = Some("l'hôte a fermé la partie".to_string());
                    break;
                }
                Ok(_) => continue,
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    closed = Some(format!("connexion perdue ({e})"));
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
        if let Some(reason) = closed {
            self.lose(reason);
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

/// Couleur d'un joueur, stable d'une partie à l'autre.
pub fn player_color(id: u16) -> Color {
    let hue = (id as f32 * 137.508) % 360.0;
    Color::hsl(hue, 0.65, 0.55)
}

/// Applique ce que le serveur a décidé.
#[allow(clippy::too_many_arguments)]
pub fn receive(
    mut d: Destroyer,
    mut lobby: ResMut<Lobby>,
    mut toasts: ResMut<crate::ui::Toasts>,
    font: Res<crate::ui::UiFont>,
    bomb_assets: Res<BombAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut remotes: Query<(Entity, &RemotePlayer, &mut Transform)>,
    tags: Query<(Entity, &NameTag)>,
    bombs: Query<(Entity, &crate::weapon::Bomb, &Transform), Without<RemotePlayer>>,
) {
    if !d.net.is_online() {
        // Hors ligne : plus personne d'autre dans le monde.
        for (entity, _, _) in &remotes {
            d.commands.entity(entity).despawn();
        }
        for (entity, _) in &tags {
            d.commands.entity(entity).despawn();
        }
        return;
    }

    for packet in d.net.drain() {
        match packet {
            Packet::Welcome {
                id,
                seed,
                mode,
                started,
            } => {
                d.net.id = Some(id);
                lobby.active = true;
                lobby.started = started;
                lobby.mode = if mode == 1 { Mode::Normal } else { Mode::Sandbox };
                // Tout le monde joue sur la carte du serveur.
                d.game.seed = seed;
                d.game.mode = lobby.mode;
                d.game.reset_inventory();
                d.world.rebuild(seed);
            }
            Packet::Refused { reason } => {
                d.net.disconnect();
                d.net.lost = Some(format!("entrée refusée : {reason}"));
            }
            Packet::Roster { host, players } => {
                let me = d.net.id;
                for (id, name) in &players {
                    if Some(*id) != me && lobby.active && !lobby.players.iter().any(|(p, _)| p == id) {
                        toasts.push(format!("{name} a rejoint la partie"));
                    }
                }
                for (id, name) in &lobby.players {
                    if !players.iter().any(|(p, _)| p == id) {
                        toasts.push(format!("{name} est parti"));
                    }
                }
                lobby.host_id = host;
                lobby.players = players;
            }
            Packet::Start => {
                lobby.started = true;
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
                let rotation = Quat::from_rotation_y(yaw);
                match remotes.iter_mut().find(|(_, r, _)| r.id == id) {
                    Some((_, _, mut t)) => {
                        // Lissage : les positions arrivent 20 fois/s.
                        t.translation = t.translation.lerp(pos, 0.6);
                        t.rotation = rotation;
                    }
                    None => {
                        let body = materials.add(StandardMaterial {
                            base_color: player_color(id),
                            perceptual_roughness: 0.7,
                            ..default()
                        });
                        let skin = materials.add(StandardMaterial {
                            base_color: Color::srgb(0.93, 0.78, 0.64),
                            perceptual_roughness: 0.8,
                            ..default()
                        });
                        let dark = materials.add(StandardMaterial {
                            base_color: Color::srgb(0.12, 0.12, 0.14),
                            ..default()
                        });
                        let torso = meshes.add(Cuboid::new(0.55, 0.75, 0.32));
                        let head = meshes.add(Cuboid::new(0.36, 0.36, 0.36));
                        let leg = meshes.add(Cuboid::new(0.22, 0.8, 0.24));
                        let arm = meshes.add(Cuboid::new(0.16, 0.7, 0.18));
                        let visor = meshes.add(Cuboid::new(0.28, 0.08, 0.02));
                        let _ = pitch;
                        d.commands
                            .spawn((
                                RemotePlayer { id },
                                Transform::from_translation(pos).with_rotation(rotation),
                                Visibility::default(),
                            ))
                            .with_children(|p| {
                                p.spawn((Mesh3d(torso), MeshMaterial3d(body.clone()), Transform::from_xyz(0.0, 1.17, 0.0)));
                                p.spawn((Mesh3d(head), MeshMaterial3d(skin), Transform::from_xyz(0.0, 1.74, 0.0)));
                                p.spawn((Mesh3d(visor), MeshMaterial3d(dark.clone()), Transform::from_xyz(0.0, 1.78, -0.185)));
                                for x in [-0.14, 0.14] {
                                    p.spawn((Mesh3d(leg.clone()), MeshMaterial3d(dark.clone()), Transform::from_xyz(x, 0.4, 0.0)));
                                }
                                for x in [-0.36, 0.36] {
                                    p.spawn((Mesh3d(arm.clone()), MeshMaterial3d(body.clone()), Transform::from_xyz(x, 1.15, 0.0)));
                                }
                            });
                        let name = lobby
                            .players
                            .iter()
                            .find(|(p, _)| *p == id)
                            .map(|(_, n)| n.clone())
                            .unwrap_or_else(|| format!("Joueur {id}"));
                        d.commands.spawn((
                            NameTag { id },
                            Node {
                                position_type: PositionType::Absolute,
                                padding: UiRect::axes(px(8.0), px(2.0)),
                                border_radius: BorderRadius::all(px(4.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                            crate::ui::text(&font, name, 14.0, Color::WHITE),
                            Visibility::Hidden,
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
                for (entity, tag) in &tags {
                    if tag.id == id {
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

/// Place les noms au-dessus des joueurs distants.
pub fn update_nametags(
    camera: Query<(&Camera, &GlobalTransform), With<crate::player::WorldCamera>>,
    remotes: Query<(&RemotePlayer, &GlobalTransform)>,
    lobby: Res<Lobby>,
    game: Res<GameState>,
    mut tags: Query<(&NameTag, &mut Node, &mut Visibility, &mut Text, &ComputedNode)>,
) {
    let Ok((cam, cam_transform)) = camera.single() else {
        return;
    };
    for (tag, mut node, mut visibility, mut text, computed) in &mut tags {
        let target = remotes.iter().find(|(r, _)| r.id == tag.id);
        let screen = target.and_then(|(_, t)| {
            let head = t.translation() + Vec3::Y * 2.2;
            let dist = head.distance(cam_transform.translation());
            if dist > 80.0 {
                return None;
            }
            cam.world_to_viewport(cam_transform, head).ok()
        });
        let wanted = match screen {
            Some(_) if game.in_game() => Visibility::Inherited,
            _ => Visibility::Hidden,
        };
        visibility.set_if_neq(wanted);
        if let Some(pos) = screen {
            let size = computed.size() * computed.inverse_scale_factor();
            node.left = px(pos.x - size.x * 0.5);
            node.top = px(pos.y - size.y);
        }
        if let Some((_, name)) = lobby.players.iter().find(|(id, _)| *id == tag.id) {
            if text.0 != *name {
                text.0 = name.clone();
            }
        }
    }
}
