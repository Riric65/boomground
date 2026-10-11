//! Serveur authoritatif (`boomground-server`).
//!
//! Il ne rejoue **pas** toute la physique : il vérifie que les actions sont
//! *cohérentes* (cadence, portée, rayon) puis applique la destruction dans
//! sa propre grille et diffuse les voxels réellement détruits. Le client
//! prédit en local, donc la latence perçue reste faible.
//!
//! La carte est générée à partir d'une seed (`BOOM_SEED`, sinon au hasard)
//! envoyée à chaque joueur, avec l'historique des destructions : un joueur
//! qui arrive en cours de partie voit la ville dans son état actuel.

use std::collections::HashMap;
use std::io::{ErrorKind, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;

use crate::game::{random_seed, WeaponId, ALL_WEAPONS};
use crate::net::protocol::{read_into, FrameReader, HitKind, Packet, MAX_CELLS_PER_PACKET};
use crate::world::VoxelWorld;

/// Distance maximale entre le joueur connu et l'origine d'un tir.
const MAX_ORIGIN_DRIFT: f32 = 6.0;
/// Distance maximale d'une explosion au joueur (une C4 se déclenche de loin).
const MAX_EXPLOSION_DISTANCE: f32 = 150.0;
/// Jetons d'action (seau qui se remplit) : tolère une rafale de plombs
/// mais pas un flood.
const BUCKET: f32 = 24.0;
const REFILL_PER_SEC: f32 = 30.0;
/// Coupure si le client ne lit plus (tampon d'envoi trop gros).
const MAX_OUTBOX: usize = 8 << 20;

struct Client {
    stream: TcpStream,
    reader: FrameReader,
    outbox: Vec<u8>,
    name: String,
    pos: Vec3,
    /// Première position reçue (on l'accepte telle quelle).
    placed: bool,
    yaw: f32,
    pitch: f32,
    weapon: u8,
    tokens: f32,
    last_refill: Instant,
    joined: bool,
}

impl Client {
    fn queue(&mut self, data: &[u8]) {
        self.outbox.extend_from_slice(data);
    }

    /// Envoie ce qui peut l'être sans bloquer. `false` = client perdu.
    fn flush(&mut self) -> bool {
        let mut sent = 0;
        while sent < self.outbox.len() {
            match self.stream.write(&self.outbox[sent..]) {
                Ok(0) => return false,
                Ok(n) => sent += n,
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => return false,
            }
        }
        self.outbox.drain(..sent);
        self.outbox.len() < MAX_OUTBOX
    }
}

#[derive(Resource)]
struct ServerState {
    listener: TcpListener,
    clients: HashMap<u16, Client>,
    next_id: u16,
    world: VoxelWorld,
    seed: u64,
    /// Tous les voxels détruits depuis le début (pour les retardataires).
    history: Vec<[u16; 3]>,
    rejections: u32,
    last_log: f32,
    last_snapshot: f32,
}

/// Ouvre le port d'écoute (`BOOM_PORT`, sinon le port par défaut).
fn bind() -> Result<(TcpListener, u16), String> {
    let port = std::env::var("BOOM_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(crate::net::DEFAULT_PORT);
    let listener = TcpListener::bind(("0.0.0.0", port))
        .map_err(|e| format!("impossible d'écouter sur le port {port} : {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    Ok((listener, port))
}

impl ServerState {
    fn new(listener: TcpListener, seed: u64) -> Self {
        let mut world = VoxelWorld::new();
        world.rebuild(seed);
        info!("BoomGround serveur : seed {seed}, {} voxels", world.alive);

        Self {
            listener,
            clients: HashMap::new(),
            next_id: 1,
            world,
            seed,
            history: Vec::new(),
            rejections: 0,
            last_log: 0.0,
            last_snapshot: 0.0,
        }
    }
}

fn server_app(listener: TcpListener, seed: u64, log: bool) -> App {
    let mut app = App::new();
    // 60 mises à jour par seconde : sans ça, la boucle tourne à vide à
    // 100 % d'un cœur.
    app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
        1.0 / 60.0,
    ))));
    if log {
        app.add_plugins(bevy::log::LogPlugin::default());
    }
    app.insert_resource(ServerState::new(listener, seed))
        .add_systems(Update, (accept_clients, serve).chain());
    app
}

/// Lancement du serveur dédié (binaire `boomground-server`).
pub fn run() {
    let seed = std::env::var("BOOM_SEED")
        .ok()
        .and_then(|v| crate::game::parse_seed(&v))
        .unwrap_or_else(random_seed);
    let (listener, port) = match bind() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("BoomGround serveur — port {port}, seed {seed}");
    server_app(listener, seed, true).run();
}

/// Héberge une partie depuis le jeu (bouton « HÉBERGER ») : le serveur
/// tourne dans un fil d'exécution à part, sans fenêtre ni terminal.
/// Renvoie le port d'écoute.
pub fn host_in_background(seed: u64) -> Result<u16, String> {
    let (listener, port) = bind()?;
    std::thread::Builder::new()
        .name("boomground-server".into())
        .spawn(move || {
            server_app(listener, seed, false).run();
        })
        .map_err(|e| e.to_string())?;
    Ok(port)
}

fn accept_clients(mut server: ResMut<ServerState>) {
    loop {
        match server.listener.accept() {
            Ok((stream, addr)) => {
                stream.set_nonblocking(true).ok();
                stream.set_nodelay(true).ok();
                let id = server.next_id;
                server.next_id = server.next_id.wrapping_add(1).max(1);
                info!("connexion de {addr} → #{id}");
                let mut client = Client {
                    stream,
                    reader: FrameReader::default(),
                    outbox: Vec::new(),
                    name: format!("joueur {id}"),
                    pos: Vec3::ZERO,
                    placed: false,
                    yaw: 0.0,
                    pitch: 0.0,
                    weapon: 0,
                    tokens: BUCKET,
                    last_refill: Instant::now(),
                    joined: false,
                };
                // Seed puis état actuel de la ville.
                client.queue(&Packet::Welcome { id, seed: server.seed }.encode());
                for cells in server.history.chunks(MAX_CELLS_PER_PACKET) {
                    client.queue(
                        &Packet::Voxels {
                            cells: cells.to_vec(),
                            rubble: false,
                        }
                        .encode(),
                    );
                }
                server.clients.insert(id, client);
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
    let now = time.elapsed_secs();
    if now - server.last_log > 10.0 {
        server.last_log = now;
        info!(
            "joueurs={} voxels={} rejets={}",
            server.clients.len(),
            server.world.alive,
            server.rejections
        );
    }

    // Diffusion des positions, 20 fois par seconde.
    if now - server.last_snapshot > 0.05 {
        server.last_snapshot = now;
        let ids: Vec<u16> = server.clients.keys().copied().collect();
        for id in ids {
            let snapshot: Vec<Vec<u8>> = server
                .clients
                .iter()
                .filter(|(&other, c)| other != id && c.joined && c.placed)
                .map(|(&other, c)| {
                    Packet::Player {
                        id: other,
                        pos: c.pos,
                        yaw: c.yaw,
                        pitch: c.pitch,
                        weapon: c.weapon,
                    }
                    .encode()
                })
                .collect();
            if let Some(client) = server.clients.get_mut(&id) {
                for data in &snapshot {
                    client.queue(data);
                }
            }
        }
    }

    let mut gone = Vec::new();
    for id in server.clients.keys().copied().collect::<Vec<_>>() {
        let Some(client) = server.clients.get_mut(&id) else {
            continue;
        };

        // Lecture de tout ce qui est disponible.
        let mut alive = true;
        loop {
            match read_into(&mut client.stream, &mut client.reader) {
                Ok(0) => {
                    alive = false;
                    break;
                }
                Ok(_) => continue,
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => {
                    alive = false;
                    break;
                }
            }
        }
        let frames: Vec<Vec<u8>> = client.reader.drain();
        if !alive {
            gone.push(id);
            continue;
        }
        for frame in frames {
            if let Some(packet) = Packet::decode(&frame) {
                handle(&mut server, id, packet);
            }
        }
    }

    for client in server.clients.values_mut() {
        if !client.flush() {
            client.joined = false;
        }
    }
    let lost: Vec<u16> = server
        .clients
        .iter()
        .filter(|(_, c)| c.outbox.len() >= MAX_OUTBOX)
        .map(|(&id, _)| id)
        .collect();
    gone.extend(lost);

    for id in gone {
        if server.clients.remove(&id).is_some() {
            info!("déconnexion de #{id}");
            broadcast(&mut server, None, &Packet::PlayerLeft { id });
        }
    }
}

/// Prend un jeton d'action (anti-flood). `false` = refusé.
fn take_token(client: &mut Client, cost: f32) -> bool {
    let now = Instant::now();
    let dt = now.duration_since(client.last_refill).as_secs_f32();
    client.last_refill = now;
    client.tokens = (client.tokens + dt * REFILL_PER_SEC).min(BUCKET);
    if client.tokens < cost {
        return false;
    }
    client.tokens -= cost;
    true
}

/// Rayon maximal accepté par type d'action (le plus grand de l'arsenal).
fn max_blast(kind: HitKind) -> f32 {
    ALL_WEAPONS
        .iter()
        .filter(|w| {
            let k = w.kind();
            match kind {
                HitKind::Sledge => k == crate::game::Kind::Melee,
                HitKind::Bullet => k == crate::game::Kind::Gun && **w != WeaponId::Rocket,
                HitKind::Explosion => w.is_bomb() || **w == WeaponId::Rocket,
            }
        })
        .map(|w| w.spec().radius)
        .fold(0.0, f32::max)
}

fn max_reach(kind: HitKind) -> f32 {
    match kind {
        HitKind::Sledge => 5.5,
        HitKind::Bullet => 170.0,
        HitKind::Explosion => 0.0,
    }
}

fn handle(server: &mut ServerState, id: u16, packet: Packet) {
    match packet {
        Packet::Join { name } => {
            if let Some(c) = server.clients.get_mut(&id) {
                c.name = name.chars().take(16).collect();
                c.joined = true;
                info!("#{id} rejoint la partie ({})", c.name);
            }
        }
        Packet::State {
            pos,
            yaw,
            pitch,
            weapon,
        } => {
            if let Some(c) = server.clients.get_mut(&id) {
                // Anti-triche : pas de téléportation (le vol libre du bac à
                // sable reste sous ~35 m/s ; 20 états par seconde).
                if !c.placed || pos.distance(c.pos) <= 4.0 {
                    c.pos = pos;
                    c.placed = true;
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
            let cost = if kind == HitKind::Explosion { 4.0 } else { 1.0 };
            if !client.placed || !take_token(client, cost) {
                server.rejections += 1;
                return;
            }
            let player = client.pos + Vec3::Y * 1.6;
            let blast = blast.clamp(0.05, max_blast(kind));

            let center = if kind == HitKind::Explosion {
                if origin.distance(player) > MAX_EXPLOSION_DISTANCE {
                    server.rejections += 1;
                    return;
                }
                origin
            } else {
                // L'origine doit être près du joueur connu, et le rayon
                // doit réellement toucher quelque chose.
                if origin.distance(player) > MAX_ORIGIN_DRIFT {
                    server.rejections += 1;
                    return;
                }
                match server.world.raycast(origin, dir, max_reach(kind)) {
                    Some(hit) => hit.point,
                    None => {
                        server.rejections += 1;
                        return;
                    }
                }
            };

            let destroyed = server.world.carve(center, blast);
            if !destroyed.is_empty() {
                let cells: Vec<[u16; 3]> = destroyed
                    .iter()
                    .map(|c| [c.x as u16, c.y as u16, c.z as u16])
                    .collect();
                server.history.extend_from_slice(&cells);
                for chunk in cells.chunks(MAX_CELLS_PER_PACKET) {
                    broadcast(
                        server,
                        None,
                        &Packet::Voxels {
                            cells: chunk.to_vec(),
                            rubble: true,
                        },
                    );
                }
            }
            if kind == HitKind::Explosion {
                broadcast(server, Some(id), &Packet::Explode { at: center, radius: blast });
            }
        }
        Packet::BombPlaced { at, weapon } => {
            if let Some(client) = server.clients.get(&id) {
                if at.distance(client.pos) <= 10.0 {
                    broadcast(server, Some(id), &Packet::BombPlaced { at, weapon });
                }
            }
        }
        _ => {}
    }
}

/// Envoie un paquet à tous les joueurs (sauf `except`).
fn broadcast(server: &mut ServerState, except: Option<u16>, packet: &Packet) {
    let data = packet.encode();
    for (&id, client) in server.clients.iter_mut() {
        if Some(id) != except {
            client.queue(&data);
        }
    }
}
