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
    /// Position de l'image précédente (vitesse pour l'animation de marche).
    pub last: Vec3,
    /// Phase du balancement des bras et des jambes.
    pub stride: f32,
}

/// Bras ou jambe d'un joueur distant, pivotant à l'épaule / la hanche.
#[derive(Component)]
pub struct Limb {
    /// +1 / -1 : les membres opposés se balancent à contretemps.
    pub side: f32,
    pub arm: bool,
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
    /// Numéro du dernier tir (les plombs d'un même tir le partagent).
    pub shot: u16,
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
    /// Partie solo (serveur privé local) : le jeu peut la mettre en pause.
    pub solo: bool,
    /// Infos du serveur reçues à l'entrée.
    pub server_name: String,
    pub max_players: u8,
    pub allow_fly: bool,
    pub chat: bool,
    pub voice: bool,
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
    // Dans le lobby ou le menu, la caméra survole la ville : ce n'est pas
    // la position du joueur, on ne l'envoie pas (sinon les autres le
    // voyaient flotter dans le ciel).
    if game.phase != crate::game::Phase::Playing {
        return;
    }
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
    let shot = net.shot;
    net.send_raw(Packet::Hit {
        kind,
        origin,
        dir,
        blast,
        shot,
    });
}

/// Couleur d'un joueur, stable d'une partie à l'autre.
pub fn player_color(id: u16) -> Color {
    let hue = (id as f32 * 137.508) % 360.0;
    Color::hsl(hue, 0.65, 0.55)
}

/// Génération de la carte en arrière-plan + paquets mis en attente.
#[derive(Resource, Default)]
pub struct WorldLoading {
    rx: Option<std::sync::Mutex<std::sync::mpsc::Receiver<crate::world::VoxelWorld>>>,
    backlog: Vec<Packet>,
    /// Date d'installation de la carte (début du maillage).
    pub installed_at: Option<std::time::Instant>,
    /// Voxels modifiés reçus du serveur pendant le chargement (affichage).
    pub edits: usize,
    /// Début de la génération de la ville (affichage).
    pub started_at: Option<std::time::Instant>,
}

/// Ressources et requêtes supplémentaires de [`receive`] (Bevy limite à 16
/// le nombre de paramètres d'un système).
#[derive(bevy::ecs::system::SystemParam)]
pub struct ReceiveExtras<'w, 's> {
    mirror: ResMut<'w, crate::net::mirror::Mirror>,
    net_blocks: Query<'w, 's, &'static mut crate::net::mirror::NetBlock>,
    net_bodies: Query<'w, 's, &'static mut crate::net::mirror::NetBody>,
    camera: Query<'w, 's, &'static GlobalTransform, With<crate::player::WorldCamera>>,
    particles: Res<'w, crate::fx::ParticleAssets>,
    density: Res<'w, crate::fx::ParticleDensity>,
    puffs: Query<'w, 's, (), With<crate::fx::Puff>>,
    wheels: ResMut<'w, crate::vehicle::WheelPoses>,
    water: ResMut<'w, crate::water::WaterMirror>,
    weather: ResMut<'w, crate::nature::Weather>,
    tornadoes: ResMut<'w, crate::nature::Tornadoes>,
    push: ResMut<'w, crate::nature::PlayerPush>,
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
    ragdolls: Query<(Entity, &crate::vitals::Ragdoll)>,
    mut fx_params: ReceiveExtras,
    mut loading: ResMut<WorldLoading>,
    mut chat: ResMut<crate::chat::Chat>,
    time: Res<Time>,
    mut voice_in: ResMut<crate::voice::VoiceInbox>,
) {
    let chat_time = time.elapsed_secs();
    let ReceiveExtras {
        ref mut mirror,
        ref mut net_blocks,
        ref mut net_bodies,
        ref camera,
        ref particles,
        ref density,
        ref puffs,
        ref mut wheels,
        ref mut water,
        ref mut weather,
        ref mut tornadoes,
        ref mut push,
    } = fx_params;
    let camera_pos = camera.single().map(|c| c.translation()).unwrap_or(Vec3::ZERO);
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

    // Carte en cours de génération (fil séparé) : les paquets suivants
    // attendent, ils s'appliquent à la nouvelle carte.
    let mut queue: std::collections::VecDeque<Packet> = std::mem::take(&mut loading.backlog).into();
    if let Some(rx) = &loading.rx {
        let got = rx.lock().ok().and_then(|r| r.try_recv().ok());
        match got {
            Some(mut new_world) => {
                new_world.generation = d.world.generation.wrapping_add(1);
                *d.world = new_world;
                loading.rx = None;
                loading.installed_at = Some(std::time::Instant::now());
            }
            None => {
                loading.backlog = queue.into();
                loading.backlog.extend(d.net.drain());
                return;
            }
        }
    }
    queue.extend(d.net.drain());

    while let Some(packet) = queue.pop_front() {
        match packet {
            Packet::Welcome {
                id,
                seed,
                mode,
                started,
                map_blocks,
                name,
                max_players,
                flags,
            } => {
                d.game.multi = !lobby.solo;
                water.cells.clear();
                water.dirty.clear();
                tornadoes.0.clear();
                weather.rain = 0.0;
                d.game.fly_allowed = flags & 1 != 0;
                lobby.server_name = name;
                lobby.max_players = max_players;
                lobby.allow_fly = flags & 1 != 0;
                lobby.chat = flags & 2 != 0;
                lobby.voice = flags & 4 != 0;
                d.net.id = Some(id);
                lobby.active = true;
                lobby.started = started;
                lobby.mode = if mode == 1 { Mode::Normal } else { Mode::Sandbox };
                // Tout le monde joue sur la carte du serveur.
                d.game.seed = seed;
                d.game.mode = lobby.mode;
                d.game.reset_inventory();
                // Génération de la ville sans bloquer l'affichage.
                let (tx, rx) = std::sync::mpsc::channel();
                let _ = std::thread::Builder::new().name("boomground-ville".into()).spawn(move || {
                    let mut w = crate::world::VoxelWorld::with_blocks(map_blocks as i32);
                    w.rebuild(seed);
                    let _ = tx.send(w);
                });
                loading.rx = Some(std::sync::Mutex::new(rx));
                loading.installed_at = None;
                loading.edits = 0;
                loading.started_at = Some(std::time::Instant::now());
                d.game.loading = true;
                d.game.loading_progress = 0.0;
                loading.backlog = queue.drain(..).collect();
                break;
            }
            Packet::Refused { reason } => {
                d.net.disconnect();
                d.net.lost = Some(format!("entrée refusée : {reason}"));
            }
            Packet::Roster { host, players } => {
                let me = d.net.id;
                for (id, name) in &players {
                    if Some(*id) != me && lobby.active && !lobby.players.iter().any(|(p, _)| p == id) {
                        toasts.push(crate::i18n::tf("{} a rejoint la partie", &[&name]));
                    }
                }
                for (id, name) in &lobby.players {
                    if !players.iter().any(|(p, _)| p == id) {
                        toasts.push(crate::i18n::tf("{} est parti", &[&name]));
                    }
                }
                lobby.host_id = host;
                lobby.players = players;
            }
            Packet::Start => {
                lobby.started = true;
            }
            Packet::Voxels { cells, .. } => {
                if d.game.loading {
                    loading.edits += cells.len();
                }
                // Autorité du serveur : on applique (idempotent : un voxel
                // déjà cassé en local est ignoré). Les gravats arrivent par
                // `Blocks`.
                let mut sum = Vec3::ZERO;
                let mut n = 0;
                for c in &cells {
                    let cell = IVec3::new(c[0] as i32, c[1] as i32, c[2] as i32);
                    if d.world.is_solid(cell) {
                        d.world.set(cell, false, 0);
                        sum += d.world.cell_center(cell);
                        n += 1;
                    }
                }
                // Gros écroulement : nuage de poussière.
                if n >= 25 {
                    let at = sum / n as f32;
                    let existing = puffs.iter().len();
                    crate::fx::dust(&mut d.commands, &particles, density.0, existing, at, n as f32, false);
                    if n >= 60 {
                        let vol = (0.5 + n as f32 / 800.0).min(1.4);
                        crate::fx::play_at(&mut d.commands, &d.bank, crate::fx::SfxKind::Rumble, vol, at);
                    }
                }
            }
            Packet::Blocks { list } => {
                mirror.add_blocks(&mut d.commands, &d.assets, &list);
            }
            Packet::BlockPose { list } => {
                mirror.pose_blocks(&mut d.commands, &list, net_blocks);
            }
            Packet::BlockRemove { ids } => {
                mirror.remove_blocks(&mut d.commands, &ids);
            }
            Packet::BodyDef { id, dims, com, part, total, data } => {
                mirror.body_part(&mut d.commands, &mut meshes, &d.assets, id, dims, com, part, total, data);
            }
            Packet::BodyPose { list } => {
                mirror.pose_bodies(&list, net_bodies);
            }
            Packet::Water { add, remove } => {
                let conv = |v: &[[u16; 3]]| v.iter().map(|c| IVec3::new(c[0] as i32, c[1] as i32, c[2] as i32)).collect::<Vec<_>>();
                water.apply(&conv(&add), &conv(&remove));
            }
            Packet::Weather { rain } => {
                weather.rain = rain as f32 / 255.0;
            }
            Packet::Tornadoes { list } => {
                tornadoes.0 = list;
            }
            Packet::Push { vel } => {
                push.0 += vel;
            }
            Packet::Wheels { list } => {
                for (id, centers, steer, spin) in list {
                    wheels.0.insert(id, (centers, steer, spin));
                }
            }
            Packet::BodyRemove { id } => {
                if let Some(e) = mirror.body_entity(id) {
                    if let Ok(b) = net_bodies.get(e) {
                        let existing = puffs.iter().len();
                        crate::fx::dust(&mut d.commands, &particles, density.0, existing, b.target_pos, 240.0, false);
                        crate::fx::play_at(&mut d.commands, &d.bank, crate::fx::SfxKind::Rumble, 1.3, b.target_pos);
                    }
                }
                mirror.remove_body(&mut d.commands, id);
            }
            Packet::Inv { money, earned, slots } => {
                d.game.money = money;
                d.game.earned = earned;
                for (i, w) in crate::game::ALL_WEAPONS.iter().enumerate() {
                    if let Some(v) = slots.get(i) {
                        let slot = d.game.slot_mut(*w);
                        slot.owned = v[0] != 0;
                        slot.mag = v[1];
                        slot.reserve = v[2];
                    }
                }
                if !d.game.slot(d.game.weapon).owned {
                    if let Some(&first) = d.game.owned().first() {
                        d.game.weapon = first;
                    }
                }
            }
            Packet::Notice { text } => {
                toasts.push(text);
            }
            Packet::Kicked { reason } => {
                d.net.lost = Some(reason);
            }
            Packet::Teleport { pos } => {
                d.game.teleport_to = Some(pos);
            }
            Packet::Impact { at, power } => {
                let kind = if power > 9.0 { crate::fx::SfxKind::BigBoom } else { crate::fx::SfxKind::Boom };
                crate::fx::play_at(&mut d.commands, &d.bank, kind, (0.3 + power * 0.04).min(1.0), at);
                crate::fx::play_at(&mut d.commands, &d.bank, crate::fx::SfxKind::Crash, (0.4 + power * 0.05).min(1.2), at);
                d.shake.add((0.05 * power / (1.0 + at.distance(camera_pos) * 0.15)).min(0.5));
                let existing = puffs.iter().len();
                crate::fx::dust(&mut d.commands, &particles, density.0, existing, at, power * 14.0, false);
            }
            Packet::Fire { add, remove } => {
                for c in remove {
                    let cell = IVec3::new(c[0] as i32, c[1] as i32, c[2] as i32);
                    if let Some(e) = mirror.fires.remove(&cell) {
                        d.commands.entity(e).despawn();
                    }
                }
                for c in add {
                    let cell = IVec3::new(c[0] as i32, c[1] as i32, c[2] as i32);
                    if mirror.fires.contains_key(&cell) || mirror.fires.len() >= crate::fx::MAX_FLAMES {
                        continue;
                    }
                    let at = d.world.cell_center(cell);
                    let phase = (cell.x * 31 + cell.y * 17 + cell.z * 7) as f32 * 0.137;
                    let e = crate::fx::flame(&mut d.commands, &particles, at, phase);
                    mirror.fires.insert(cell, e);
                }
            }
            Packet::ChatMsg { from, name, text } => {
                let line = if from == 0 { text } else { format!("<{name}> {text}") };
                let now = chat_time;
                chat.push(line, now, from == 0);
            }
            // Pas de voix sur téléphone : personne ne viderait la file.
            Packet::Voice { from, data, .. } => {
                if !crate::touch::is_touch_platform() {
                    voice_in.push((from, data));
                }
            }
            Packet::Driving { body } => {
                d.game.driving = body;
            }
            Packet::Reset { seed } => {
                water.cells.clear();
                water.dirty.clear();
                tornadoes.0.clear();
                weather.rain = 0.0;
                d.game.seed = seed;
                let blocks = d.world.blocks;
                let (tx, rx) = std::sync::mpsc::channel();
                let _ = std::thread::Builder::new().name("boomground-ville".into()).spawn(move || {
                    let mut w = crate::world::VoxelWorld::with_blocks(blocks);
                    w.rebuild(seed);
                    let _ = tx.send(w);
                });
                loading.rx = Some(std::sync::Mutex::new(rx));
                loading.installed_at = None;
                d.game.loading = true;
                d.game.loading_progress = 0.0;
                loading.backlog = queue.drain(..).collect();
                break;
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
                        // Personnage trapu et contrasté : lisible de face comme
                        // de profil, même de loin.
                        let torso = meshes.add(Cuboid::new(0.62, 0.78, 0.38));
                        let head = meshes.add(Cuboid::new(0.44, 0.44, 0.44));
                        let leg = meshes.add(Cuboid::new(0.27, 0.82, 0.32));
                        let arm = meshes.add(Cuboid::new(0.2, 0.74, 0.24));
                        let visor = meshes.add(Cuboid::new(0.34, 0.1, 0.03));
                        let jeans = materials.add(StandardMaterial {
                            base_color: Color::srgb(0.18, 0.24, 0.42),
                            perceptual_roughness: 0.85,
                            ..default()
                        });
                        let _ = pitch;
                        d.commands
                            .spawn((
                                RemotePlayer {
                                    id,
                                    last: pos,
                                    stride: 0.0,
                                },
                                Transform::from_translation(pos).with_rotation(rotation),
                                Visibility::default(),
                            ))
                            .with_children(|p| {
                                p.spawn((Mesh3d(torso), MeshMaterial3d(body.clone()), Transform::from_xyz(0.0, 1.21, 0.0)));
                                p.spawn((Mesh3d(head), MeshMaterial3d(skin), Transform::from_xyz(0.0, 1.84, 0.0)));
                                p.spawn((Mesh3d(visor), MeshMaterial3d(dark.clone()), Transform::from_xyz(0.0, 1.88, -0.225)));
                                // Jambes : pivot à la hanche.
                                for (x, side) in [(-0.16, 1.0), (0.16, -1.0)] {
                                    p.spawn((Limb { side, arm: false }, Transform::from_xyz(x, 0.82, 0.0), Visibility::default()))
                                        .with_child((Mesh3d(leg.clone()), MeshMaterial3d(jeans.clone()), Transform::from_xyz(0.0, -0.41, 0.0)));
                                }
                                // Bras : pivot à l'épaule.
                                for (x, side) in [(-0.42, -1.0), (0.42, 1.0)] {
                                    p.spawn((Limb { side, arm: true }, Transform::from_xyz(x, 1.56, 0.0), Visibility::default()))
                                        .with_child((Mesh3d(arm.clone()), MeshMaterial3d(body.clone()), Transform::from_xyz(0.0, -0.36, 0.0)));
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
            Packet::Placed { cells } => {
                if d.game.loading {
                    loading.edits += cells.len();
                }
                for c in cells {
                    let cell = IVec3::new(c[0] as i32, c[1] as i32, c[2] as i32);
                    if c[3] == 255 {
                        d.world.set(cell, false, 0);
                    } else {
                        d.world.set(cell, true, c[3] as u8);
                    }
                }
            }
            Packet::DoorState { door, open } => {
                let index = door as usize;
                if let Some(door) = d.world.doors.get_mut(index) {
                    door.is_open = open;
                    let at = d.world.doors[index].center(&d.world);
                    crate::fx::play_at(&mut d.commands, &d.bank, crate::fx::SfxKind::Door, 0.8, at);
                }
            }
            Packet::Hurt { hp, from } => {
                // La secousse est gérée par `vitals::feedback`, à la mesure
                // des points perdus (avant : une forte secousse fixe à chaque
                // petit dégât, la caméra sautait au contact d'une voiture).
                d.game.hp = hp as f32;
                let _ = from;
            }
            Packet::Died { id, at, push } => {
                if Some(id) == d.net.id {
                    d.game.hp = 0.0;
                    d.game.dead = true;
                    d.game.respawn_in = crate::game::RESPAWN_TIME;
                    d.game.death_push = push;
                } else {
                    crate::vitals::spawn_ragdoll(
                        &mut d.commands,
                        &mut meshes,
                        &mut materials,
                        at,
                        push,
                        player_color(id),
                        id,
                        false,
                    );
                    for (entity, remote, _) in &remotes {
                        if remote.id == id {
                            d.commands.entity(entity).insert(Visibility::Hidden);
                        }
                    }
                }
            }
            Packet::Respawned { id, pos } => {
                if Some(id) == d.net.id {
                    d.game.respawn_at = Some(pos);
                } else {
                    for (entity, remote, mut t) in &mut remotes {
                        if remote.id == id {
                            t.translation = pos;
                            d.commands.entity(entity).insert(Visibility::Inherited);
                        }
                    }
                    for (entity, r) in &ragdolls {
                        if r.owner == id && !r.local {
                            d.commands.entity(entity).despawn();
                        }
                    }
                }
            }
            // Paquets destinés au serveur : ignorés côté client.
            Packet::Join { .. }
            | Packet::State { .. }
            | Packet::Hit { .. }
            | Packet::Place { .. }
            | Packet::Interact { .. }
            | Packet::Reload
            | Packet::Buy { .. }
            | Packet::Throw { .. }
            | Packet::Command { .. }
            | Packet::Chat { .. }
            | Packet::Status
            | Packet::StatusInfo { .. }
            | Packet::Drive { .. }
            | Packet::Enter { .. }
            | Packet::Exit
            | Packet::Ping => {}
        }
    }
}

/// Place les noms au-dessus des joueurs distants.
pub fn update_nametags(
    camera: Query<(&Camera, &GlobalTransform), With<crate::player::WorldCamera>>,
    remotes: Query<(&RemotePlayer, &GlobalTransform)>,
    lobby: Res<Lobby>,
    game: Res<GameState>,
    ui_scale: Res<bevy::ui::UiScale>,
    mut tags: Query<(&NameTag, &mut Node, &mut Visibility, &mut Text, &ComputedNode)>,
) {
    let Ok((cam, cam_transform)) = camera.single() else {
        return;
    };
    // L'interface est mise à l'échelle par `UiScale` : ses coordonnées ne
    // sont pas les pixels logiques de la fenêtre mais ceux divisés par
    // l'échelle. Sans cette division les noms partaient en diagonale vers le
    // coin de l'écran au lieu de rester au-dessus des joueurs.
    let scale = ui_scale.0.max(0.01);
    let eye = cam_transform.translation();
    let forward = cam_transform.forward().normalize_or_zero();
    for (tag, mut node, mut visibility, mut text, computed) in &mut tags {
        let target = remotes.iter().find(|(r, _)| r.id == tag.id);
        let screen = target.and_then(|(_, t)| {
            let head = t.translation() + Vec3::Y * 2.35;
            let to = head - eye;
            let dist = to.length();
            if dist > 80.0 || dist < 0.4 {
                return None;
            }
            // Derrière la caméra : pas de nom (sinon il se retrouve
            // projeté par symétrie dans le coin opposé de l'écran).
            if forward.dot(to / dist) <= 0.1 {
                return None;
            }
            let pos = cam.world_to_viewport(cam_transform, head).ok()?;
            let Some(rect) = cam.logical_viewport_rect() else {
                return None;
            };
            if pos.x < rect.min.x - 4.0 || pos.x > rect.max.x + 4.0 || pos.y < rect.min.y - 4.0 || pos.y > rect.max.y + 4.0 {
                return None;
            }
            Some(pos)
        });
        let wanted = match screen {
            Some(_) if game.in_game() => Visibility::Inherited,
            _ => Visibility::Hidden,
        };
        visibility.set_if_neq(wanted);
        if let Some(pos) = screen {
            let size = computed.size() * computed.inverse_scale_factor();
            node.left = px(pos.x / scale - size.x * 0.5);
            node.top = px(pos.y / scale - size.y);
        }
        if let Some((_, name)) = lobby.players.iter().find(|(id, _)| *id == tag.id) {
            if text.0 != *name {
                text.0 = name.clone();
            }
        }
    }
}

/// Balancement des bras et des jambes des joueurs distants quand ils
/// avancent.
pub fn animate_remotes(
    time: Res<Time>,
    mut remotes: Query<(&mut RemotePlayer, &Transform, &Children)>,
    mut limbs: Query<(&Limb, &mut Transform), Without<RemotePlayer>>,
) {
    let dt = time.delta_secs().max(1e-4);
    for (mut remote, transform, children) in &mut remotes {
        let moved = transform.translation - remote.last;
        remote.last = transform.translation;
        let speed = Vec2::new(moved.x, moved.z).length() / dt;
        let amount = (speed / 5.0).min(1.0);
        remote.stride += speed * dt * 1.8;
        for child in children.iter() {
            if let Ok((limb, mut t)) = limbs.get_mut(child) {
                let swing = remote.stride.sin() * 0.7 * amount * limb.side;
                let angle = if limb.arm { -swing * 0.8 } else { swing };
                // Retour en douceur à la position de repos à l'arrêt.
                let target = Quat::from_rotation_x(angle);
                t.rotation = t.rotation.slerp(target, (dt * 12.0).min(1.0));
            }
        }
    }
}
