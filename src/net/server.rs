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
    read_into, FrameReader, HitKind, Packet, DEFAULT_PORT, MAX_CELLS_PER_PACKET,
    MAX_PLACED_PER_PACKET, MAX_PLAYERS, PROTOCOL_VERSION,
};
use crate::net::upnp;
use crate::world::VoxelWorld;
use bevy::math::IVec3;

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

/// Règles d'une partie (modifiables dans `server.conf`).
#[derive(Clone, Debug)]
pub struct Rules {
    /// Nombre maximal de joueurs (1..=16).
    pub max_players: usize,
    /// Les joueurs peuvent se blesser entre eux.
    pub pvp: bool,
    /// Multiplicateur de dégâts (0 = invulnérable).
    pub damage_scale: f32,
    /// Secondes avant la réapparition.
    pub respawn: f32,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            max_players: MAX_PLAYERS,
            pvp: true,
            damage_scale: 1.0,
            respawn: crate::game::RESPAWN_TIME,
        }
    }
}

/// Réglages d'une partie hébergée.
#[derive(Clone, Debug)]
pub struct HostConfig {
    pub rules: Rules,
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
    /// Dernière position acceptée, et positions refusées d'affilée.
    last_state: Instant,
    rejected: u32,
    /// Connexion acceptée depuis moins de 10 s sans `Join` : on patiente.
    since: Instant,
    hp: f32,
    /// Mort : réapparition à cette date.
    dead_until: Option<Instant>,
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
    /// Toutes les modifications du monde depuis le début, dans l'ordre
    /// `[x, y, z, matériau]` (255 = voxel retiré), pour les retardataires.
    history: Vec<[u16; 4]>,
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

/// Contenu du `server.conf` généré au premier lancement.
const CONF_TEMPLATE: &str = "\
# BoomGround — configuration du serveur dédié
# Modifie ce fichier puis relance le serveur. Les lignes qui commencent par
# # sont des commentaires. Supprime le fichier pour le régénérer.

# Nom affiché dans la liste des parties (réseau local).
name = Serveur BoomGround

# Port TCP d'écoute (ouvre-le sur ta box / ton pare-feu pour jouer en ligne).
port = 28431

# Graine de la ville : un nombre, un mot, ou « random » (nouvelle ville à
# chaque démarrage).
seed = random

# Mode de jeu : sandbox (tout débloqué) ou normal (argent et boutique).
mode = sandbox

# Nombre maximal de joueurs (1 à 16).
max_players = 16

# Les joueurs peuvent se blesser entre eux (true / false). Les explosions
# restent dangereuses pour leur auteur.
pvp = true

# Multiplicateur de dégâts : 1 = normal, 0 = invulnérable, 2 = double.
damage_scale = 1.0

# Secondes avant la réapparition d'un joueur mort.
respawn_seconds = 5
";

/// Réglages lus dans `server.conf`.
struct ServerConf {
    name: String,
    port: u16,
    seed: Option<u64>,
    mode: u8,
    rules: Rules,
}

/// Emplacement du fichier : à côté de l'exécutable (sinon dossier courant).
fn conf_path() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("server.conf")))
        .unwrap_or_else(|| "server.conf".into())
}

/// Lit `server.conf` (le crée s'il n'existe pas).
fn load_conf() -> ServerConf {
    let path = conf_path();
    if !path.exists() {
        match std::fs::write(&path, CONF_TEMPLATE) {
            Ok(()) => println!("server.conf créé : {}", path.display()),
            Err(e) => eprintln!("impossible de créer {} : {e}", path.display()),
        }
    }
    let mut conf = ServerConf {
        name: "Serveur BoomGround".into(),
        port: DEFAULT_PORT,
        seed: None,
        mode: 0,
        rules: Rules::default(),
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return conf;
    };
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            eprintln!("server.conf ligne {} ignorée : {line}", n + 1);
            continue;
        };
        let (key, value) = (key.trim().to_lowercase(), value.trim());
        match key.as_str() {
            "name" if !value.is_empty() => conf.name = value.chars().take(40).collect(),
            "port" => conf.port = value.parse().unwrap_or(conf.port),
            "seed" => {
                conf.seed = if value.eq_ignore_ascii_case("random") {
                    None
                } else {
                    crate::game::parse_seed(value)
                }
            }
            "mode" => conf.mode = u8::from(value.eq_ignore_ascii_case("normal")),
            "max_players" => {
                conf.rules.max_players = value.parse().unwrap_or(conf.rules.max_players).clamp(1, MAX_PLAYERS)
            }
            "pvp" => conf.rules.pvp = !value.eq_ignore_ascii_case("false"),
            "damage_scale" => {
                conf.rules.damage_scale = value.parse().unwrap_or(conf.rules.damage_scale).clamp(0.0, 10.0)
            }
            "respawn_seconds" => {
                conf.rules.respawn = value.parse().unwrap_or(conf.rules.respawn).clamp(1.0, 60.0)
            }
            other => eprintln!("server.conf : clé inconnue « {other} »"),
        }
    }
    conf
}

/// Serveur dédié (`boomground-server`, ou `BoomGround --server`). Les
/// variables `BOOM_PORT`, `BOOM_SEED`, `BOOM_MODE` et `BOOM_NAME` priment
/// sur `server.conf`.
pub fn run_dedicated() {
    let conf = load_conf();
    let seed = std::env::var("BOOM_SEED")
        .ok()
        .and_then(|v| crate::game::parse_seed(&v))
        .or(conf.seed)
        .unwrap_or_else(random_seed);
    let mode = match std::env::var("BOOM_MODE") {
        Ok(m) => u8::from(m == "normal"),
        Err(_) => conf.mode,
    };
    let port = std::env::var("BOOM_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(conf.port);
    let listeners = match bind(port) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let name = std::env::var("BOOM_NAME").unwrap_or(conf.name);
    println!(
        "BoomGround — serveur dédié « {name} » : port {port}, seed {seed}, mode {}, {} joueurs max, pvp {}",
        if mode == 1 { "normal" } else { "bac à sable" },
        conf.rules.max_players,
        conf.rules.pvp
    );
    println!("Génération de la ville…");
    let config = HostConfig {
        rules: conf.rules,
        seed,
        mode,
        name,
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
        rules: Rules::default(),
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
                last_state: Instant::now(),
                rejected: 0,
                since: Instant::now(),
                hp: 100.0,
                dead_until: None,
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

    // Réapparitions.
    let due: Vec<u16> = server
        .clients
        .iter()
        .filter(|(_, c)| c.dead_until.is_some_and(|t| Instant::now() >= t))
        .map(|(&id, _)| id)
        .collect();
    for id in due {
        let pos = crate::player::spawn_point(&server.world, Some(id));
        if let Some(c) = server.clients.get_mut(&id) {
            c.hp = 100.0;
            c.dead_until = None;
            c.pos = pos;
            c.last_state = Instant::now();
        }
        broadcast(&mut server, None, &Packet::Respawned { id, pos });
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
                .filter(|(&other, c)| {
                    other != id && c.joined && c.placed && c.dead_until.is_none()
                })
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

/// Distance d'entrée du rayon dans la boîte d'un joueur (pieds en `feet`).
fn ray_box(origin: Vec3, dir: Vec3, feet: Vec3, max: f32) -> Option<f32> {
    let lo = feet + Vec3::new(-0.35, 0.0, -0.35);
    let hi = feet + Vec3::new(0.35, 1.8, 0.35);
    let mut t0 = 0.0f32;
    let mut t1 = max;
    for a in 0..3 {
        if dir[a].abs() < 1e-6 {
            if origin[a] < lo[a] || origin[a] > hi[a] {
                return None;
            }
        } else {
            let (mut n, mut f) = ((lo[a] - origin[a]) / dir[a], (hi[a] - origin[a]) / dir[a]);
            if n > f {
                std::mem::swap(&mut n, &mut f);
            }
            t0 = t0.max(n);
            t1 = t1.min(f);
            if t0 > t1 {
                return None;
            }
        }
    }
    Some(t0)
}

/// Inflige des dégâts à un joueur ; le tue si ses points de vie tombent à 0.
fn hurt(server: &mut ServerState, victim: u16, by: Option<u16>, amount: f32, from: Vec3, push: Vec3) {
    let rules = server.config.rules.clone();
    if !rules.pvp && by != Some(victim) {
        return;
    }
    let amount = amount * rules.damage_scale;
    if amount <= 0.0 {
        return;
    }
    let Some(c) = server.clients.get_mut(&victim) else {
        return;
    };
    if c.dead_until.is_some() {
        return;
    }
    c.hp = (c.hp - amount).max(0.0);
    let hp = c.hp;
    let at = c.pos;
    let dead = hp <= 0.0;
    if dead {
        c.dead_until = Some(Instant::now() + Duration::from_secs_f32(rules.respawn.max(1.0)));
    }
    send(server, victim, &Packet::Hurt { hp: hp.ceil() as u8, from });
    if dead {
        info!("#{victim} est mort (par {by:?})");
        broadcast(server, None, &Packet::Died { id: victim, at, push });
    }
}

/// Dégâts d'une explosion : décroissent avec la distance.
fn explosion_damage(server: &mut ServerState, by: u16, center: Vec3, radius: f32) {
    let reach = radius * 2.2 + 1.0;
    let victims: Vec<(u16, Vec3)> = server
        .clients
        .iter()
        .filter(|(_, c)| c.joined && c.placed && c.dead_until.is_none())
        .map(|(&id, c)| (id, c.pos + Vec3::Y * 0.9))
        .collect();
    for (victim, pos) in victims {
        let d = pos.distance(center);
        if d >= reach {
            continue;
        }
        let amount = 150.0 * (1.0 - d / reach).powf(1.2);
        let away = (pos - center).normalize_or_zero();
        hurt(server, victim, Some(by), amount, center, away * (6.0 + radius * 2.0) + Vec3::Y * 5.0);
    }
}

/// Historique → paquets (suites de retraits `Voxels`, suites de poses
/// `Placed`, dans l'ordre).
fn encode_history(history: &[[u16; 4]]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < history.len() {
        let removing = history[i][3] == 255;
        let mut j = i;
        while j < history.len() && (history[j][3] == 255) == removing {
            j += 1;
        }
        if removing {
            for chunk in history[i..j].chunks(MAX_CELLS_PER_PACKET) {
                let cells = chunk.iter().map(|c| [c[0], c[1], c[2]]).collect();
                out.push(Packet::Voxels { cells, rubble: false }.encode());
            }
        } else {
            for chunk in history[i..j].chunks(MAX_PLACED_PER_PACKET) {
                out.push(Packet::Placed { cells: chunk.to_vec() }.encode());
            }
        }
        i = j;
    }
    out
}

fn max_reach(kind: HitKind) -> f32 {
    match kind {
        HitKind::Sledge => 5.5,
        HitKind::Bullet => 230.0,
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
            if joined >= server.config.rules.max_players.clamp(1, MAX_PLAYERS) {
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
            let history = encode_history(&server.history);
            let doors: Vec<Vec<u8>> = server
                .world
                .doors
                .iter()
                .enumerate()
                .filter(|(_, d)| d.is_open)
                .map(|(i, _)| Packet::DoorState { door: i as u16, open: true }.encode())
                .collect();
            if let Some(c) = server.clients.get_mut(&id) {
                c.name = unique.clone();
                c.joined = true;
                c.queue(&welcome.encode());
                for data in history.iter().chain(&doors) {
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
                if c.dead_until.is_some() {
                    return;
                }
                // Anti-triche : vitesse bornée (le vol libre du bac à sable
                // reste sous ~35 m/s). Une position refusée en continu
                // pendant une seconde est un vrai saut (réapparition,
                // explosion) : on se recale dessus au lieu de bloquer le
                // joueur pour toujours à son ancienne place.
                let now = Instant::now();
                let dt = now.duration_since(c.last_state).as_secs_f32().min(1.0);
                let allowed = 4.0 + 45.0 * dt;
                if !c.placed || pos.distance(c.pos) <= allowed || c.rejected >= 20 {
                    c.pos = pos;
                    c.placed = true;
                    c.last_state = now;
                    c.rejected = 0;
                } else {
                    c.rejected += 1;
                    server.rejections += 1;
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
            if !client.placed
                || !client.joined
                || client.dead_until.is_some()
                || !take_token(client, cost)
            {
                server.rejections += 1;
                return;
            }
            let player = client.pos + Vec3::Y * 1.6;
            let weapon = WeaponId::from_tag(client.weapon);
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
                let reach = max_reach(kind);
                let world_hit = server.world.raycast(origin, dir, reach);
                let limit = world_hit.as_ref().map_or(reach, |h| h.point.distance(origin));
                // Un joueur sur la trajectoire est blessé.
                let dmg = weapon.spec().damage;
                if dmg > 0.0 {
                    let dir_n = dir.normalize_or_zero();
                    let target = server
                        .clients
                        .iter()
                        .filter(|(&other, c)| {
                            other != id && c.joined && c.placed && c.dead_until.is_none()
                        })
                        .filter_map(|(&other, c)| {
                            ray_box(origin, dir_n, c.pos, limit.min(if kind == HitKind::Sledge { 3.5 } else { reach }))
                                .map(|t| (other, t))
                        })
                        .min_by(|a, b| a.1.total_cmp(&b.1));
                    if let Some((victim, _)) = target {
                        hurt(server, victim, Some(id), dmg, origin, dir_n * 4.0);
                    }
                }
                match world_hit {
                    Some(hit) => hit.point,
                    None => {
                        if dmg <= 0.0 {
                            server.rejections += 1;
                        }
                        return;
                    }
                }
            };

            let mut destroyed = server.world.carve(center, blast);
            if !destroyed.is_empty() {
                let fallen = server.world.collapse(&destroyed);
                destroyed.extend(fallen);
            }
            if !destroyed.is_empty() {
                let cells: Vec<[u16; 3]> = destroyed
                    .iter()
                    .map(|c| [c.x as u16, c.y as u16, c.z as u16])
                    .collect();
                server
                    .history
                    .extend(cells.iter().map(|c| [c[0], c[1], c[2], 255]));
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
                explosion_damage(server, id, center, blast);
            }
        }
        Packet::Place { cell, material } => {
            if !server.started {
                return;
            }
            let Some(client) = server.clients.get_mut(&id) else {
                return;
            };
            if !client.placed || !client.joined || client.dead_until.is_some() || !take_token(client, 1.0) {
                server.rejections += 1;
                return;
            }
            let at = IVec3::new(cell[0] as i32, cell[1] as i32, cell[2] as i32);
            let center = server.world.cell_center(at);
            let near = center.distance(client.pos + Vec3::Y * 1.6) <= 14.0;
            let d = server.world.dims;
            let inside = at.y >= 1 && at.cmpge(IVec3::ZERO).all() && at.cmplt(d).all();
            if !near || !inside || server.world.is_solid(at) || material as usize >= crate::world::PALETTE.len() {
                server.rejections += 1;
                return;
            }
            server.world.set(at, true, material);
            let edit = [cell[0], cell[1], cell[2], material as u16];
            server.history.push(edit);
            broadcast(server, None, &Packet::Placed { cells: vec![edit] });
        }
        Packet::Interact { door } => {
            if !server.started {
                return;
            }
            let Some(client) = server.clients.get_mut(&id) else {
                return;
            };
            if !client.placed || !client.joined || client.dead_until.is_some() || !take_token(client, 2.0) {
                return;
            }
            let pos = client.pos + Vec3::Y * 1.6;
            let index = door as usize;
            let Some(d) = server.world.doors.get(index) else {
                return;
            };
            if d.center(&server.world).distance(pos) > 7.0 {
                server.rejections += 1;
                return;
            }
            if let Some(edits) = server.world.toggle_door(index) {
                server.history.extend_from_slice(&edits);
                let open = server.world.doors[index].is_open;
                for chunk in edits.chunks(MAX_PLACED_PER_PACKET) {
                    broadcast(server, None, &Packet::Placed { cells: chunk.to_vec() });
                }
                broadcast(server, None, &Packet::DoorState { door, open });
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
