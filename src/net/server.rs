//! Serveur de partie : lobby, puis autorité sur la destruction.
//!
//! Quand on **crée un lobby** depuis le menu, ce serveur tourne en fond dans
//! le jeu lui-même (aucune fenêtre, aucun terminal) :
//! - il écoute sur le port 28431 (IPv4 et IPv6) ;
//! - il répond aux menus du réseau local (la partie apparaît toute seule) ;
//! - il demande à la box d'ouvrir le port (UPnP) pour que les joueurs
//!   d'Internet puissent entrer avec le code de partie.
//!
//! Il ne rejoue **pas** la physique : il vérifie que les actions sont
//! cohérentes (cadence, portée, rayon), applique la destruction dans sa
//! propre grille et diffuse les voxels réellement détruits. Un joueur qui
//! arrive en cours de partie reçoit la seed et l'historique des destructions.
//!
//! `BoomGround --server` lance le même serveur sans fenêtre (serveur dédié).

use std::collections::HashMap;
use std::io::{ErrorKind, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::prelude::*;

use crate::game::{random_seed, WeaponId, ALL_WEAPONS};
use crate::net::lan::{LobbyInfo, Responder};
use crate::net::protocol::{
    read_into, FrameReader, HitKind, Packet, DEFAULT_PORT, MAX_CELLS_PER_PACKET, MAX_PLAYERS,
    PROTOCOL_VERSION,
};
use crate::net::upnp;
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

/// Réglages d'une partie hébergée.
#[derive(Clone, Debug)]
pub struct HostConfig {
    pub seed: u64,
    /// 0 = bac à sable, 1 = normal.
    pub mode: u8,
    /// Nom affiché dans la liste des parties.
    pub name: String,
    /// Jeton secret de l'hôte (lui seul peut lancer la partie).
    pub token: u64,
    /// Serveur dédié : pas d'hôte, partie lancée d'office.
    pub dedicated: bool,
}

/// Accès depuis Internet.
#[derive(Clone, Debug, PartialEq)]
pub enum Internet {
    /// Recherche de la box en cours.
    Checking,
    /// Port ouvert : les joueurs d'Internet peuvent entrer.
    Open(Ipv4Addr),
    /// Port ouvert mais la box est derrière un autre NAT (CGNAT).
    Shared(Ipv4Addr),
    /// Ouverture impossible (raison).
    Closed(String),
}

/// État de l'hébergement, partagé avec le menu.
#[derive(Debug)]
pub struct HostStatus {
    pub lan_ip: Option<Ipv4Addr>,
    pub internet: Internet,
    gateway: Option<upnp::Gateway>,
}

/// Partie hébergée par ce jeu. La lâcher arrête le serveur et referme le
/// port de la box.
pub struct HostHandle {
    pub port: u16,
    pub token: u64,
    pub status: Arc<Mutex<HostStatus>>,
    shutdown: Arc<AtomicBool>,
}

impl HostHandle {
    pub fn internet(&self) -> Internet {
        self.status
            .lock()
            .map(|s| s.internet.clone())
            .unwrap_or(Internet::Checking)
    }

    pub fn lan_ip(&self) -> Option<Ipv4Addr> {
        self.status.lock().ok().and_then(|s| s.lan_ip)
    }
}

impl Drop for HostHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // On referme la box tout de suite (le jeu peut quitter juste après).
        let gateway = self.status.lock().ok().and_then(|mut s| s.gateway.take());
        if let Some(gw) = gateway {
            gw.close(self.port);
        }
    }
}

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
    /// Connexion acceptée depuis moins de 10 s sans `Join` : on patiente.
    since: Instant,
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
    listeners: Vec<TcpListener>,
    port: u16,
    clients: HashMap<u16, Client>,
    next_id: u16,
    world: VoxelWorld,
    config: HostConfig,
    started: bool,
    host: Option<u16>,
    /// Tous les voxels détruits depuis le début (pour les retardataires).
    history: Vec<[u16; 3]>,
    rejections: u32,
    last_log: f32,
    last_snapshot: f32,
    responder: Responder,
    shutdown: Arc<AtomicBool>,
}

/// Ouvre le port d'écoute (IPv4, et IPv6 si disponible).
fn bind(port: u16) -> Result<Vec<TcpListener>, String> {
    let v4 = TcpListener::bind(("0.0.0.0", port)).map_err(|e| {
        if e.kind() == ErrorKind::AddrInUse {
            format!("le port {port} est déjà utilisé (une autre partie est-elle ouverte ?)")
        } else {
            format!("impossible d'écouter sur le port {port} : {e}")
        }
    })?;
    let mut listeners = vec![v4];
    if let Ok(v6) = TcpListener::bind(("::", port)) {
        listeners.push(v6);
    }
    for l in &listeners {
        l.set_nonblocking(true).map_err(|e| e.to_string())?;
    }
    Ok(listeners)
}

fn env_port() -> u16 {
    std::env::var("BOOM_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

fn server_app(listeners: Vec<TcpListener>, port: u16, config: HostConfig, shutdown: Arc<AtomicBool>, log: bool) -> App {
    let mut world = VoxelWorld::new();
    world.rebuild(config.seed);
    let mut app = App::new();
    // 60 mises à jour par seconde : sans ça, la boucle tourne à vide à
    // 100 % d'un cœur.
    app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
        1.0 / 60.0,
    ))));
    if log {
        app.add_plugins(bevy::log::LogPlugin::default());
    }
    app.insert_resource(ServerState {
        listeners,
        port,
        clients: HashMap::new(),
        next_id: 1,
        world,
        started: config.dedicated,
        config,
        host: None,
        history: Vec::new(),
        rejections: 0,
        last_log: 0.0,
        last_snapshot: 0.0,
        responder: Responder::new(),
        shutdown,
    })
    .add_systems(Update, (stop_if_asked, accept_clients, serve).chain());
    app
}

/// Serveur dédié (`BoomGround --server`).
pub fn run_dedicated() {
    let seed = std::env::var("BOOM_SEED")
        .ok()
        .and_then(|v| crate::game::parse_seed(&v))
        .unwrap_or_else(random_seed);
    let mode = if std::env::var("BOOM_MODE").is_ok_and(|m| m == "normal") { 1 } else { 0 };
    let port = env_port();
    let listeners = match bind(port) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    println!("BoomGround — serveur dédié, port {port}, seed {seed}");
    let config = HostConfig {
        seed,
        mode,
        name: std::env::var("BOOM_NAME").unwrap_or_else(|_| "Serveur BoomGround".into()),
        token: 0,
        dedicated: true,
    };
    server_app(listeners, port, config, Arc::new(AtomicBool::new(false)), true).run();
}

/// Crée un lobby : serveur en fond + ouverture du port de la box.
pub fn host_in_background(seed: u64, mode: u8, name: String) -> Result<HostHandle, String> {
    let port = env_port();
    let listeners = bind(port)?;
    let token = random_seed() ^ (random_seed() << 20) | 1;
    let shutdown = Arc::new(AtomicBool::new(false));
    let status = Arc::new(Mutex::new(HostStatus {
        lan_ip: crate::net::lan::local_ipv4(),
        internet: Internet::Checking,
        gateway: None,
    }));

    let config = HostConfig {
        seed,
        mode,
        name,
        token,
        dedicated: false,
    };
    let stop = shutdown.clone();
    std::thread::Builder::new()
        .name("boomground-serveur".into())
        .spawn(move || {
            server_app(listeners, port, config, stop, false).run();
        })
        .map_err(|e| e.to_string())?;

    // Ouverture du port sur la box, sans bloquer le jeu.
    let shared = status.clone();
    let stop = shutdown.clone();
    let _ = std::thread::Builder::new()
        .name("boomground-upnp".into())
        .spawn(move || {
            let result = upnp::discover().and_then(|gw| {
                gw.open(port)?;
                let ip = gw.external_ip()?;
                Ok((gw, ip))
            });
            let Ok(mut s) = shared.lock() else { return };
            match result {
                Ok((gw, ip)) => {
                    if stop.load(Ordering::SeqCst) {
                        // Lobby déjà refermé pendant la recherche.
                        gw.close(port);
                        return;
                    }
                    s.lan_ip = Some(gw.local_ip);
                    s.internet = if upnp::is_shared_address(ip) {
                        Internet::Shared(ip)
                    } else {
                        Internet::Open(ip)
                    };
                    s.gateway = Some(gw);
                }
                Err(e) => s.internet = Internet::Closed(e),
            }
        });

    Ok(HostHandle {
        port,
        token,
        status,
        shutdown,
    })
}

fn stop_if_asked(server: Res<ServerState>, mut exit: MessageWriter<AppExit>) {
    if server.shutdown.load(Ordering::SeqCst) {
        exit.write(AppExit::Success);
    }
}

fn accept_clients(mut server: ResMut<ServerState>) {
    let mut accepted = Vec::new();
    for listener in &server.listeners {
        loop {
            match listener.accept() {
                Ok((stream, addr)) => accepted.push((stream, addr)),
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => {
                    error!("accept : {e}");
                    break;
                }
            }
        }
    }
    for (stream, addr) in accepted {
        stream.set_nonblocking(true).ok();
        stream.set_nodelay(true).ok();
        let id = server.next_id;
        server.next_id = server.next_id.wrapping_add(1).max(1);
        info!("connexion de {addr} → #{id}");
        server.clients.insert(
            id,
            Client {
                stream,
                reader: FrameReader::default(),
                outbox: Vec::new(),
                name: format!("Joueur {id}"),
                pos: Vec3::ZERO,
                placed: false,
                yaw: 0.0,
                pitch: 0.0,
                weapon: 0,
                tokens: BUCKET,
                last_refill: Instant::now(),
                joined: false,
                since: Instant::now(),
            },
        );
    }
}

fn serve(mut server: ResMut<ServerState>, time: Res<Time>) {
    let now = time.elapsed_secs();
    if now - server.last_log > 30.0 {
        server.last_log = now;
        info!(
            "joueurs={} voxels={} rejets={}",
            server.clients.len(),
            server.world.alive,
            server.rejections
        );
    }

    // Réponse aux menus du réseau local.
    let info = LobbyInfo {
        name: server.config.name.clone(),
        port: server.port,
        players: server.clients.values().filter(|c| c.joined).count() as u8,
        max: MAX_PLAYERS as u8,
        mode: server.config.mode,
        started: server.started,
    };
    server.responder.poll(&info);

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
                if client.joined {
                    for data in &snapshot {
                        client.queue(data);
                    }
                }
            }
        }
    }

    let mut gone = Vec::new();
    for id in server.clients.keys().copied().collect::<Vec<_>>() {
        let Some(client) = server.clients.get_mut(&id) else {
            continue;
        };

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
        if !client.joined && client.since.elapsed() > Duration::from_secs(10) {
            alive = false;
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

    for (&id, client) in server.clients.iter_mut() {
        if !client.flush() {
            gone.push(id);
        }
    }

    let mut roster_changed = false;
    for id in gone {
        if let Some(client) = server.clients.remove(&id) {
            info!("départ de #{id}");
            if client.joined {
                roster_changed = true;
                broadcast(&mut server, None, &Packet::PlayerLeft { id });
            }
        }
    }
    if roster_changed {
        send_roster(&mut server);
    }
}

fn send_roster(server: &mut ServerState) {
    let mut players: Vec<(u16, String)> = server
        .clients
        .iter()
        .filter(|(_, c)| c.joined)
        .map(|(&id, c)| (id, c.name.clone()))
        .collect();
    players.sort_by_key(|(id, _)| *id);
    let host = server.host.unwrap_or(0);
    broadcast(server, None, &Packet::Roster { host, players });
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

/// Envoie un paquet à un seul joueur.
fn send(server: &mut ServerState, id: u16, packet: &Packet) {
    if let Some(c) = server.clients.get_mut(&id) {
        c.queue(&packet.encode());
    }
}

fn handle(server: &mut ServerState, id: u16, packet: Packet) {
    match packet {
        Packet::Join {
            name,
            token,
            version,
        } => {
            if server.clients.get(&id).is_none_or(|c| c.joined) {
                return;
            }
            if version != PROTOCOL_VERSION {
                send(
                    server,
                    id,
                    &Packet::Refused {
                        reason: "version du jeu différente : mettez BoomGround à jour".into(),
                    },
                );
                return;
            }
            let joined = server.clients.values().filter(|c| c.joined).count();
            if joined >= MAX_PLAYERS {
                send(server, id, &Packet::Refused { reason: "la partie est pleine".into() });
                return;
            }
            // Nom propre et unique.
            let base: String = name
                .chars()
                .filter(|c| !c.is_control())
                .take(16)
                .collect::<String>()
                .trim()
                .to_string();
            let base = if base.is_empty() { format!("Joueur {id}") } else { base };
            let mut unique = base.clone();
            let mut n = 2;
            while server.clients.values().any(|c| c.joined && c.name == unique) {
                unique = format!("{base} ({n})");
                n += 1;
            }
            if !server.config.dedicated && token == server.config.token && server.host.is_none() {
                server.host = Some(id);
            }

            let welcome = Packet::Welcome {
                id,
                seed: server.config.seed,
                mode: server.config.mode,
                started: server.started,
            };
            let history: Vec<Vec<u8>> = server
                .history
                .chunks(MAX_CELLS_PER_PACKET)
                .map(|cells| {
                    Packet::Voxels {
                        cells: cells.to_vec(),
                        rubble: false,
                    }
                    .encode()
                })
                .collect();
            if let Some(c) = server.clients.get_mut(&id) {
                c.name = unique.clone();
                c.joined = true;
                c.queue(&welcome.encode());
                for data in &history {
                    c.queue(data);
                }
            }
            info!("#{id} rejoint la partie ({unique})");
            send_roster(server);
        }
        Packet::Start => {
            if server.host == Some(id) && !server.started {
                server.started = true;
                info!("la partie commence");
                broadcast(server, None, &Packet::Start);
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
            if !server.started {
                return;
            }
            let Some(client) = server.clients.get_mut(&id) else {
                return;
            };
            let cost = if kind == HitKind::Explosion { 4.0 } else { 1.0 };
            if !client.placed || !client.joined || !take_token(client, cost) {
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
                if server.started && at.distance(client.pos) <= 10.0 {
                    broadcast(server, Some(id), &Packet::BombPlaced { at, weapon });
                }
            }
        }
        _ => {}
    }
}

/// Envoie un paquet à tous les joueurs entrés (sauf `except`).
fn broadcast(server: &mut ServerState, except: Option<u16>, packet: &Packet) {
    let data = packet.encode();
    for (&id, client) in server.clients.iter_mut() {
        if Some(id) != except && client.joined {
            client.queue(&data);
        }
    }
}
