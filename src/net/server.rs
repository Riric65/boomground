//! Serveur authoritatif (`boomground-server`).
//!
//! Il ne rejoue **pas** toute la physique : il vérifie que les actions sont
//! *cohérentes* (cadence, portée, rayon, destruction plausible) puis applique
//! la destruction dans sa propre grille et diffuse les voxels réellement
//! détruits. Le client prédit en local, donc la latence perçue reste faible.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use bevy::prelude::*;

use crate::net::protocol::{read_into, write_packet, FrameReader, HitKind, Packet};
use crate::world::VoxelWorld;

/// Cadence minimale entre deux actions (anti-triche).
const SLEDGE_COOLDOWN: f32 = 0.25;
const BULLET_COOLDOWN: f32 = 0.07;
/// Portée maximale d'un tir depuis la position connue du joueur.
const MAX_REACH: f32 = 8.0;
/// Rayon de destruction maximal accepté par type d'action.
const MAX_BLAST_SLEDGE: f32 = 1.3;
const MAX_BLAST_BULLET: f32 = 0.35;
/// Nombre maximal de voxels qu'un joueur peut détruire par seconde.
const MAX_VOXELS_PER_SEC: u32 = 300;

/// Blocs de gravats connus du serveur (pour savoir quoi diffuser).
#[derive(Default)]
struct ChunkRegistry {
    keys: std::collections::HashSet<IVec3>,
}

struct Client {
    stream: TcpStream,
    reader: FrameReader,
    name: String,
    pos: Vec3,
    yaw: f32,
    pitch: f32,
    weapon: u8,
    last_hit: Instant,
    voxels_this_sec: u32,
    second_start: Instant,
    joined: bool,
}

#[derive(Resource)]
struct ServerState {
    listener: TcpListener,
    clients: HashMap<u16, Client>,
    next_id: u16,
    world: VoxelWorld,
    rejections: u32,
    chunks: ChunkRegistry,
}

impl Default for ServerState {
    fn default() -> Self {
        let port = std::env::var("BOOM_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(crate::net::DEFAULT_PORT);
        let listener = TcpListener::bind(("0.0.0.0", port))
            .unwrap_or_else(|e| panic!("impossible d'écouter sur {port} : {e}"));
        listener
            .set_nonblocking(true)
            .expect("socket non bloquant");

        let mut world = VoxelWorld::new();
        world.rebuild();

        Self {
            listener,
            clients: HashMap::new(),
            next_id: 1,
            world,
            rejections: 0,
            chunks: ChunkRegistry::default(),
        }
    }
}

/// Lancement du serveur (binaire `boomground-server`).
pub fn run() {
    App::new()
        .add_plugins(MinimalPlugins)
        // Le serveur n'a pas de rendu, mais il veut tout de même un journal.
        .add_plugins(bevy::log::LogPlugin::default())
        .init_resource::<ServerState>()
        .add_systems(Update, (accept_clients, serve).chain())
        .run();
}

fn accept_clients(mut server: ResMut<ServerState>) {
    loop {
        match server.listener.accept() {
            Ok((stream, addr)) => {
                stream.set_nonblocking(true).ok();
                let id = server.next_id;
                server.next_id += 1;
                info!("connexion de {addr} → #{id}");
                let mut clone = stream.try_clone().expect("clone du socket");
                let _ = write_packet(&mut clone, &Packet::Welcome { id });
                server.clients.insert(
                    id,
                    Client {
                        stream,
                        reader: FrameReader::default(),
                        name: format!("joueur {id}"),
                        pos: Vec3::ZERO,
                        yaw: 0.0,
                        pitch: 0.0,
                        weapon: 0,
                        last_hit: Instant::now() - Duration::from_secs(1),
                        voxels_this_sec: 0,
                        second_start: Instant::now(),
                        joined: false,
                    },
                );
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) => {
                error!("accept : {e}");
                break;
            }
        }
    }
}

fn serve(mut server: ResMut<ServerState>, time: Res<Time>) {
    // Journal d'état toutes les 5 s : joueurs, voxels restants, rejets.
    if time.elapsed_secs() % 5.0 < time.delta_secs() {
        info!(
            "joueurs={} voxels={} rejets={}",
            server.clients.len(),
            server.world.alive,
            server.rejections
        );
    }

    // Diffusion des états connus, 20 fois/s suffit (latence faible, trafic léger).
    if time.elapsed_secs() % 0.05 < time.delta_secs() {
        let snapshot: Vec<Packet> = server
            .clients
            .iter()
            .filter(|(_, c)| c.joined)
            .map(|(&id, c)| Packet::Player {
                id,
                pos: c.pos,
                yaw: c.yaw,
                pitch: c.pitch,
                weapon: c.weapon,
            })
            .collect();
        broadcast(&mut server, &snapshot);
    }

    for id in server.clients.keys().copied().collect::<Vec<_>>() {
        let Some(client) = server.clients.get_mut(&id) else {
            continue;
        };

        match read_into(&mut client.stream, &mut client.reader) {
            Ok(0) => {
                info!("déconnexion de #{id}");
                if server.clients.remove(&id).is_none() {
                    continue;
                };
                broadcast(&mut server, &[Packet::PlayerLeft { id }]);
                continue;
            }
            Ok(_) => {}
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(_) => {
                server.clients.remove(&id);
                continue;
            }
        }

        // On vide le tampon d'abord : le monde est ensuite modifiable.
        let frames: Vec<Vec<u8>> = client.reader.drain();
        for frame in frames {
            if let Some(packet) = Packet::decode(&frame) {
                handle(&mut server, id, packet);
            }
        }
    }
}

fn handle(server: &mut ServerState, id: u16, packet: Packet) {
    let now = Instant::now();

    match packet {
        Packet::Join { name } => {
            if let Some(c) = server.clients.get_mut(&id) {
                c.name = name.chars().take(16).collect();
                c.joined = true;
            }
            info!("#{} rejoint la partie ({})", id, server.clients[&id].name);
        }
        Packet::State {
            pos,
            yaw,
            pitch,
            weapon,
        } => {
            // Anti-triche : on borne les téléportations (20 m/s max).
            if let Some(c) = server.clients.get_mut(&id) {
                let limit = 20.0 * 0.25;
                if pos.distance(c.pos) <= limit {
                    c.pos = pos;
                }
                c.yaw = yaw;
                c.pitch = pitch;
                c.weapon = weapon;
            }
        }
        Packet::Hit {
            kind,
            origin,
            dir,
            blast,
        } => {
            let Some(client) = server.clients.get_mut(&id) else {
                return;
            };

            // 1. Cadence.
            let cooldown = match kind {
                HitKind::Sledge => SLEDGE_COOLDOWN,
                HitKind::Bullet => BULLET_COOLDOWN,
                HitKind::Explosion => return,
            };
            if now.duration_since(client.last_hit) < Duration::from_secs_f32(cooldown) {
                server.rejections += 1;
                return;
            }
            client.last_hit = now;

            // 2. Portée : le point de départ doit être près du joueur connu.
            if origin.distance(client.pos) > MAX_REACH {
                server.rejections += 1;
                return;
            }

            // 3. Rayon borné.
            let max_blast = match kind {
                HitKind::Sledge => MAX_BLAST_SLEDGE,
                HitKind::Bullet => MAX_BLAST_BULLET,
                HitKind::Explosion => return,
            };
            let blast = blast.clamp(0.05, max_blast);
            let _ = blast;

            // 4. Cohérence : le rayon doit réellement toucher quelque chose.
            let hit = server.world.raycast(origin, dir, MAX_REACH);
            let Some(hit) = hit else {
                server.rejections += 1;
                return;
            };

            // 5. Budget de destruction par seconde.
            if now.duration_since(client.second_start) > Duration::from_secs(1) {
                client.second_start = now;
                client.voxels_this_sec = 0;
            }

            let max_blast = match kind {
                HitKind::Sledge => MAX_BLAST_SLEDGE,
                HitKind::Bullet => MAX_BLAST_BULLET,
                HitKind::Explosion => 0.0,
            };
            let destroyed = server.world.carve(hit.point, blast.min(max_blast));

            let Some(client) = server.clients.get_mut(&id) else {
                return;
            };
            client.voxels_this_sec += destroyed.len() as u32;
            if client.voxels_this_sec > MAX_VOXELS_PER_SEC {
                server.rejections += 1;
                return;
            }

            let cells: Vec<[u16; 3]> = destroyed
                .iter()
                .map(|c| [c.x as u16, c.y as u16, c.z as u16])
                .collect();
            broadcast(server, &[Packet::Voxels { cells }]);

            // Gravats : on regarde quelles zones ont changé (règle
            // déterministe partagée avec le client, pas de physique ici).
            let mut spawned = Vec::new();
            let mut removed = Vec::new();
            for cell in destroyed {
                let key = crate::rubble::chunk_key(cell);
                if crate::rubble::zone_has_voxels(&server.world, key) {
                    if server.chunks.keys.insert(key) {
                        let at = crate::rubble::zone_center(&server.world, key);
                        spawned.push(crate::net::protocol::ChunkState {
                            key: [key.x as i16, key.y as i16, key.z as i16],
                            at: [at.x, at.y, at.z],
                            material: crate::rubble::zone_material(&server.world, key),
                        });
                    }
                } else if server.chunks.keys.remove(&key) {
                    removed.push([key.x as i16, key.y as i16, key.z as i16]);
                }
            }
            if !spawned.is_empty() || !removed.is_empty() {
                broadcast(server, &[Packet::Chunks { spawned, removed }]);
            }
        }
        Packet::C4Placed { at } => {
            // Cohérence : la C4 doit être près du joueur.
            if let Some(client) = server.clients.get(&id) {
                if at.distance(client.pos) <= crate::weapon::C4_REACH {
                    broadcast(server, &[Packet::C4Placed { at }]);
                }
            }
        }
        _ => {}
    }
}

fn broadcast(server: &mut ServerState, packets: &[Packet]) {
    let encoded: Vec<Vec<u8>> = packets.iter().map(|p| p.encode()).collect();
    for client in server.clients.values_mut() {
        for data in &encoded {
            if client.stream.write_bytes(data).is_err() {
                break;
            }
        }
    }
}

trait WriteBytes {
    fn write_bytes(&mut self, data: &[u8]) -> std::io::Result<()>;
}

impl WriteBytes for TcpStream {
    fn write_bytes(&mut self, data: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        self.write_all(data)
    }
}