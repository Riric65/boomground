//! Serveur de partie : l'**autorité** sur tout ce qui compte.
//!
//! Le même serveur sert :
//! - le **solo** (le jeu lance un serveur privé en fond, sur 127.0.0.1) ;
//! - les **lobbys** créés depuis le menu (réseau local + UPnP + code) ;
//! - le **serveur dédié** (`boomground-server`, réglé par `server.conf`).
//!
//! Il possède la ville, simule la physique (gravats, effondrement des
//! bâtiments, corps rigides), tient l'inventaire et l'argent de chaque
//! joueur, valide chaque action (cadence, munitions, portée, déplacement),
//! applique les dégâts, et peut sauvegarder / recharger tout le monde.

use std::collections::{BTreeMap, HashMap};
use std::io::{ErrorKind, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::app::{AppExit, ScheduleRunnerPlugin};
use bevy::prelude::*;

use crate::game::{random_seed, GameState, Kind, Mode, WeaponId, ALL_WEAPONS};
use crate::net::conf::{self, Rules};
use crate::net::lan::{LobbyInfo, Responder};
use crate::net::logger;
use crate::net::protocol::{
    read_into, BlockInfo, FrameReader, HitKind, Packet, DEFAULT_PORT, MAX_CELLS_PER_PACKET,
    MAX_PLACED_PER_PACKET, MAX_PLAYERS, PROTOCOL_VERSION,
};
use crate::net::save::{self, PlayerSave, SaveData};
use crate::net::upnp;
use crate::sim::{Sim, SimEvent, W};
use crate::world::{material_value, VoxelWorld};

/// Distance maximale entre le joueur connu et l'origine d'un tir.
const MAX_ORIGIN_DRIFT: f32 = 3.5;
/// Distance maximale d'une explosion au joueur (une C4 se déclenche de loin).
const MAX_EXPLOSION_DISTANCE: f32 = 150.0;
/// Jetons d'action (seau qui se remplit) : tolère une rafale de plombs
/// mais pas un flood.
const BUCKET: f32 = 24.0;
const REFILL_PER_SEC: f32 = 30.0;
/// Coupure si le client ne lit plus (tampon d'envoi trop gros).
const MAX_OUTBOX: usize = 8 << 20;
/// Octets de grille par paquet `BodyDef`.
const BODY_PART: usize = 30_000;
/// Fenêtre d'anti-rebond de l'anti-triche : un même motif n'est compté
/// qu'une fois par tranche de deux secondes (les états arrivent à 20 Hz).
const FLAG_WINDOW: Duration = Duration::from_secs(2);
/// Au-delà de ce nombre de refus **consécutifs**, une suspicion est notée :
/// avant, le moindre état isolé suffisait à déclencher le compteur.
const STRIKE_AFTER: u32 = 5;

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
    /// Partie solo : serveur privé, partie lancée d'office, pause possible.
    pub solo: bool,
    /// Nom du monde (fichier `.bgw`).
    pub world_name: String,
    /// Côté de la carte (pâtés de maisons).
    pub map_blocks: u16,
    pub motd: String,
    /// Dossier des sauvegardes.
    pub save_dir: PathBuf,
    /// Sauvegarde à charger.
    pub load: Option<PathBuf>,
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
    addr: IpAddr,
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
    /// Inventaire autoritaire (argent, armes, munitions).
    inv: GameState,
    inv_dirty: bool,
    last_inv: Instant,
    /// Tirs : dernier numéro, plombs restants, date.
    last_shot_id: u16,
    pellets_left: u32,
    last_shot_at: Instant,
    last_swing_at: Instant,
    reload_until: Option<Instant>,
    /// Bombes armées (rayon maximal, date) en attente d'explosion.
    pending: Vec<(f32, Instant, WeaponId)>,
    /// Anti-triche : points de suspicion, état de vol / chute.
    strikes: f32,
    /// Dernière suspicion notée (anti-rebond).
    flag_at: Option<Instant>,
    last_valid: Vec3,
    airborne_since: Option<Instant>,
    fall_speed: f32,
    grace_until: Instant,
    protected_until: Instant,
    last_activity: Instant,
    last_crush: Instant,
    /// Dernière alerte envoyée pour une arme interdite.
    last_notice: Instant,
    /// Déplacement forcé en cours : position visée et date.
    teleporting: Option<(Vec3, Instant)>,
    /// Chat : dates des derniers messages, dernier texte.
    chat_times: std::collections::VecDeque<Instant>,
    last_chat: String,
    last_chat_at: Instant,
    /// Voix : seau de jetons (trames de 20 ms).
    voice_tokens: f32,
    voice_refill: Instant,
    /// Connexion de simple consultation : fermée après la réponse.
    close_after: bool,
    /// Corps du véhicule conduit et dernières commandes.
    driving: Option<u32>,
    drive: (i8, i8, bool),
    drive_at: Instant,
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
    sim: Sim,
    events: Vec<SimEvent>,
    config: HostConfig,
    started: bool,
    paused: bool,
    host: Option<u16>,
    /// Toutes les modifications du monde depuis le début, dans l'ordre
    /// `[x, y, z, matériau]` (255 = voxel retiré), pour les retardataires.
    history: Vec<[u16; 4]>,
    /// Joueurs connus (rechargés avec le monde, mis à jour à leur départ).
    players: BTreeMap<String, PlayerSave>,
    banned: Vec<String>,
    /// Joueurs réduits au silence (pseudo en minuscules).
    muted: Vec<String>,
    /// Voiture (index de la carte) → corps rigide activé.
    car_bodies: HashMap<u16, u32>,
    rejections: u32,
    last_snapshot: f32,
    last_sim: Instant,
    last_pose: Instant,
    last_unsupported: Instant,
    unsupported_for: f32,
    last_autosave: Instant,
    last_idle_check: Instant,
    responder: Responder,
    shutdown: Arc<AtomicBool>,
    console: Option<Mutex<mpsc::Receiver<String>>>,
    /// Voxels en feu : temps de combustion restant.
    fire: HashMap<IVec3, f32>,
    last_fire: Instant,
    fire_rng: u64,
}

// ----------------------------------------------------------------------
// Création du serveur
// ----------------------------------------------------------------------

/// Ouvre le port d'écoute (IPv4, et IPv6 si disponible).
fn bind(port: u16, local_only: bool) -> Result<Vec<TcpListener>, String> {
    let host = if local_only { "127.0.0.1" } else { "0.0.0.0" };
    let v4 = TcpListener::bind((host, port)).map_err(|e| {
        if e.kind() == ErrorKind::AddrInUse {
            format!("le port {port} est déjà utilisé (une autre partie est-elle ouverte ?)")
        } else {
            format!("impossible d'écouter sur le port {port} : {e}")
        }
    })?;
    let mut listeners = vec![v4];
    if !local_only {
        if let Ok(v6) = TcpListener::bind(("::", port)) {
            listeners.push(v6);
        }
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

/// Inventaire de départ d'un joueur selon le mode et les règles.
fn fresh_inventory(rules: &Rules, mode: u8) -> GameState {
    let mut g = GameState {
        mode: if mode == 1 { Mode::Normal } else { Mode::Sandbox },
        ..default()
    };
    g.reset_inventory();
    if g.mode == Mode::Normal {
        g.money = rules.starting_money;
    }
    apply_restrictions(&mut g, rules);
    g
}

/// Retire de l'inventaire les armes interdites par les règles.
fn apply_restrictions(g: &mut GameState, rules: &Rules) {
    for &id in &ALL_WEAPONS {
        let banned = !rules.weapon_enabled(id) || (id == WeaponId::Blocks && !rules.allow_building);
        if banned {
            let s = g.slot_mut(id);
            s.owned = false;
            s.mag = 0;
            s.reserve = 0;
        }
    }
    if !g.slot(g.weapon).owned {
        g.weapon = g.owned().first().copied().unwrap_or(WeaponId::Hammer);
    }
}

fn inventory_packet(g: &GameState) -> Packet {
    Packet::Inv {
        money: g.money,
        earned: g.earned,
        slots: ALL_WEAPONS
            .iter()
            .map(|&w| {
                let s = g.slot(w);
                [s.owned as u32, s.mag, s.reserve]
            })
            .collect(),
    }
}

fn new_client(stream: TcpStream, addr: IpAddr, rules: &Rules, mode: u8) -> Client {
    let now = Instant::now();
    Client {
        stream,
        reader: FrameReader::default(),
        outbox: Vec::new(),
        addr,
        name: String::new(),
        pos: Vec3::ZERO,
        placed: false,
        yaw: 0.0,
        pitch: 0.0,
        weapon: 0,
        tokens: BUCKET,
        last_refill: now,
        joined: false,
        last_state: now,
        rejected: 0,
        since: now,
        hp: 100.0,
        dead_until: None,
        inv: fresh_inventory(rules, mode),
        inv_dirty: false,
        last_inv: now,
        last_shot_id: 0,
        pellets_left: 0,
        last_shot_at: now - Duration::from_secs(5),
        last_swing_at: now - Duration::from_secs(5),
        reload_until: None,
        pending: Vec::new(),
        strikes: 0.0,
        flag_at: None,
        last_valid: Vec3::ZERO,
        airborne_since: None,
        fall_speed: 0.0,
        grace_until: now,
        protected_until: now,
        last_activity: now,
        last_crush: now,
        last_notice: now - Duration::from_secs(10),
        teleporting: None,
        chat_times: std::collections::VecDeque::new(),
        last_chat: String::new(),
        last_chat_at: now - Duration::from_secs(10),
        voice_tokens: 100.0,
        voice_refill: now,
        close_after: false,
        driving: None,
        drive: (0, 0, false),
        drive_at: now,
    }
}

fn read_banned() -> Vec<String> {
    std::fs::read_to_string(conf::base_dir().join("banned-players.txt"))
        .map(|t| t.lines().map(|l| l.trim().to_lowercase()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

fn server_app(
    listeners: Vec<TcpListener>,
    port: u16,
    config: HostConfig,
    shutdown: Arc<AtomicBool>,
    console: Option<mpsc::Receiver<String>>,
) -> Result<App, String> {
    let mut sim = Sim::new();
    let mut world = VoxelWorld::with_blocks(config.map_blocks as i32);
    let mut history = Vec::new();
    let mut players = BTreeMap::new();
    let mut config = config;

    if let Some(path) = &config.load {
        let data = SaveData::read_file(path)?;
        if data.map_version != crate::map::MAP_VERSION {
            return Err(format!(
                "ce monde a été créé avec une autre version de la carte ({} ≠ {})",
                data.map_version,
                crate::map::MAP_VERSION
            ));
        }
        config.seed = data.seed;
        config.mode = data.mode;
        config.map_blocks = data.map_blocks;
        world = VoxelWorld::with_blocks(data.map_blocks as i32);
        world.rebuild(data.seed);
        for e in &data.history {
            let c = IVec3::new(e[0] as i32, e[1] as i32, e[2] as i32);
            if e[3] == 255 {
                world.set(c, false, 0);
            } else {
                world.set(c, true, e[3] as u8);
            }
        }
        for &d in &data.doors_open {
            if let Some(door) = world.doors.get_mut(d as usize) {
                door.is_open = true;
            }
        }
        if !data.sim.is_empty() {
            let mut r = crate::sim::R::new(&data.sim);
            sim.read(&mut r).ok_or("simulation illisible dans la sauvegarde")?;
        }
        history = data.history;
        players = data.players;
        logger::info(format!(
            "Monde « {} » chargé : seed {}, {} modifications, {} blocs, {} corps",
            data.name,
            data.seed,
            history.len(),
            sim.blocks.len(),
            sim.bodies.len()
        ));
    } else {
        world.rebuild(config.seed);
    }

    let mut app = App::new();
    // 60 mises à jour par seconde : sans ça, la boucle tourne à vide à
    // 100 % d'un cœur.
    app.add_plugins(MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 60.0))));
    let now = Instant::now();
    app.insert_resource(ServerState {
        listeners,
        port,
        clients: HashMap::new(),
        next_id: 1,
        world,
        sim,
        events: Vec::new(),
        started: config.dedicated || config.solo,
        paused: false,
        config,
        host: None,
        history,
        players,
        banned: read_banned(),
        muted: Vec::new(),
        car_bodies: HashMap::new(),
        rejections: 0,
        last_snapshot: 0.0,
        last_sim: now,
        last_pose: now,
        last_unsupported: now,
        unsupported_for: 0.0,
        last_autosave: now,
        last_idle_check: now,
        responder: Responder::new(),
        shutdown,
        console: console.map(Mutex::new),
        fire: HashMap::new(),
        last_fire: now,
        fire_rng: 0x2545_F491_4F6C_DD1D,
    })
    .add_systems(Update, (stop_if_asked, accept_clients, serve).chain());
    Ok(app)
}

/// Serveur dédié (`boomground-server`, ou `BoomGround --server`). Les
/// variables `BOOM_PORT`, `BOOM_SEED`, `BOOM_MODE` (`normal`) et `BOOM_NAME`
/// priment sur `server.conf`.
pub fn run_dedicated() {
    let conf = conf::load();
    let dir = conf::base_dir();
    logger::init(&dir.join("logs"), conf.log_level);
    logger::info(format!("BoomGround {} — serveur dédié", crate::install::VERSION));
    if !dir.join("server.conf").exists() {
        logger::warn("server.conf introuvable : réglages par défaut");
    }
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
    let listeners = match bind(port, false) {
        Ok(v) => v,
        Err(e) => {
            logger::error(e);
            std::process::exit(1);
        }
    };
    let name = std::env::var("BOOM_NAME").unwrap_or(conf.name);
    let save_dir = dir.join("worlds");
    let path = save_dir.join(format!("{}.bgw", conf.world));
    let load = path.exists().then_some(path);
    logger::info(format!(
        "Monde « {} » · {} · {} joueurs max · pvp {} · anti-triche {}",
        conf.world,
        if mode == 1 { "mode normal" } else { "bac à sable" },
        conf.rules.max_players,
        if conf.rules.pvp { "oui" } else { "non" },
        conf.rules.anticheat
    ));
    if !conf.rules.disabled_weapons.is_empty() {
        logger::info(format!(
            "Armes interdites : {}",
            conf.rules.disabled_weapons.iter().map(|w| w.key()).collect::<Vec<_>>().join(", ")
        ));
    }
    logger::info("Génération de la ville…");
    let config = HostConfig {
        rules: conf.rules,
        seed,
        mode,
        name,
        token: 0,
        dedicated: true,
        solo: false,
        world_name: conf.world,
        map_blocks: conf.map_blocks,
        motd: conf.motd,
        save_dir,
        load,
    };
    let app = server_app(listeners, port, config, Arc::new(AtomicBool::new(false)), Some(crate::net::console::start()));
    match app {
        Ok(mut app) => {
            logger::info(format!("Prêt : écoute sur le port {port}. Tape « help » pour les commandes."));
            app.run();
            crate::net::console::stop();
            logger::info("Serveur arrêté.");
        }
        Err(e) => {
            logger::error(e);
            std::process::exit(1);
        }
    }
}

/// Options d'un serveur lancé en fond depuis le jeu.
pub struct BackgroundOptions {
    pub seed: u64,
    pub mode: u8,
    pub name: String,
    /// Solo : serveur privé (127.0.0.1, port libre, pas d'UPnP).
    pub solo: bool,
    /// Monde à charger.
    pub load: Option<PathBuf>,
    pub world_name: String,
    /// Côté de la carte, en pâtés de maisons.
    pub map_blocks: u16,
}

/// Crée un serveur en fond : lobby (réseau local + UPnP) ou solo (privé).
pub fn host_in_background(opts: BackgroundOptions) -> Result<HostHandle, String> {
    let port = if opts.solo { 0 } else { env_port() };
    let listeners = bind(port, opts.solo)?;
    let port = listeners[0].local_addr().map(|a| a.port()).unwrap_or(port);
    let token = random_seed() ^ (random_seed() << 20) | 1;
    let shutdown = Arc::new(AtomicBool::new(false));
    let status = Arc::new(Mutex::new(HostStatus {
        lan_ip: crate::net::lan::local_ipv4(),
        internet: Internet::Checking,
        gateway: None,
    }));
    let save_dir = crate::game::config_dir()
        .map(|d| d.join("saves"))
        .unwrap_or_else(|| PathBuf::from("saves"));
    let config = HostConfig {
        rules: Rules::default(),
        seed: opts.seed,
        mode: opts.mode,
        name: opts.name,
        token,
        dedicated: false,
        solo: opts.solo,
        world_name: opts.world_name,
        map_blocks: opts.map_blocks,
        motd: String::new(),
        save_dir,
        load: opts.load,
    };
    // Le monde est construit dans le fil du serveur : on attend le premier
    // signe de vie pour remonter une éventuelle erreur de chargement.
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let stop = shutdown.clone();
    std::thread::Builder::new()
        .name("boomground-serveur".into())
        .spawn(move || match server_app(listeners, port, config, stop, None) {
            Ok(mut app) => {
                let _ = ready_tx.send(Ok(()));
                app.run();
            }
            Err(e) => {
                let _ = ready_tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    match ready_rx.recv_timeout(Duration::from_secs(60)) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(e),
        Err(_) => return Err("le serveur ne répond pas".into()),
    }

    if opts.solo {
        return Ok(HostHandle {
            port,
            token,
            status,
            shutdown,
        });
    }

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
                    logger::error(format!("accept : {e}"));
                    break;
                }
            }
        }
    }
    for (stream, addr) in accepted {
        stream.set_nonblocking(true).ok();
        stream.set_nodelay(true).ok();
        let ip = addr.ip();
        let same = server.clients.values().filter(|c| c.addr == ip).count();
        if !ip.is_loopback() && same >= server.config.rules.max_per_ip {
            logger::warn(format!("{ip} : trop de connexions depuis cette adresse, refusée"));
            continue;
        }
        let id = server.next_id;
        server.next_id = server.next_id.wrapping_add(1).max(1);
        logger::debug(format!("connexion de {addr} → #{id}"));
        let client = new_client(stream, ip, &server.config.rules, server.config.mode);
        let mut client = client;
        client.name = format!("Joueur {id}");
        server.clients.insert(id, client);
    }
}

// ----------------------------------------------------------------------
// Diffusion
// ----------------------------------------------------------------------

/// Envoie un paquet à tous les joueurs entrés (sauf `except`).
fn broadcast(server: &mut ServerState, except: Option<u16>, packet: &Packet) {
    let data = packet.encode();
    for (&id, client) in server.clients.iter_mut() {
        if Some(id) != except && client.joined {
            client.queue(&data);
        }
    }
}

/// Envoie un paquet à un seul joueur.
fn send(server: &mut ServerState, id: u16, packet: &Packet) {
    if let Some(c) = server.clients.get_mut(&id) {
        c.queue(&packet.encode());
    }
}

fn notice(server: &mut ServerState, id: u16, text: &str) {
    send(server, id, &Packet::Notice { text: text.to_string() });
}

/// Voxels retirés du monde : historique + diffusion.
fn broadcast_removed(server: &mut ServerState, cells: &[IVec3]) {
    if cells.is_empty() {
        return;
    }
    let cells: Vec<[u16; 3]> = cells.iter().map(|c| [c.x as u16, c.y as u16, c.z as u16]).collect();
    server.history.extend(cells.iter().map(|c| [c[0], c[1], c[2], 255]));
    for chunk in cells.chunks(MAX_CELLS_PER_PACKET) {
        broadcast(
            server,
            None,
            &Packet::Voxels {
                cells: chunk.to_vec(),
                rubble: false,
            },
        );
    }
}

fn broadcast_placed(server: &mut ServerState, edits: &[[u16; 4]]) {
    if edits.is_empty() {
        return;
    }
    server.history.extend_from_slice(edits);
    for chunk in edits.chunks(MAX_PLACED_PER_PACKET) {
        broadcast(server, None, &Packet::Placed { cells: chunk.to_vec() });
    }
}

fn block_info(b: &crate::sim::Block) -> BlockInfo {
    BlockInfo {
        id: b.id,
        pos: b.pos,
        half: b.half,
        yaw: b.yaw,
        mat: b.mat,
    }
}

/// Paquets `BodyDef` d'un corps (grille compressée, en morceaux).
fn body_def_packets(body: &crate::sim::Body) -> Vec<Vec<u8>> {
    let rle = body.grid.encode_rle();
    // Les plages font 3 octets : on découpe sur des multiples de 3.
    let step = (BODY_PART / 3) * 3;
    let total = rle.len().div_ceil(step).max(1);
    (0..total)
        .map(|i| {
            let lo = i * step;
            let hi = (lo + step).min(rle.len());
            Packet::BodyDef {
                id: body.id,
                dims: [body.grid.dims.x as u16, body.grid.dims.y as u16, body.grid.dims.z as u16],
                com: body.com,
                part: i as u16,
                total: total as u16,
                data: rle[lo..hi].to_vec(),
            }
            .encode()
        })
        .collect()
}

/// Traduit les événements de la simulation en paquets.
fn flush_events(server: &mut ServerState) {
    let events = std::mem::take(&mut server.events);
    for ev in events {
        match ev {
            SimEvent::BlocksAdded(ids) => {
                let infos: Vec<BlockInfo> = server
                    .sim
                    .blocks
                    .iter()
                    .filter(|b| ids.contains(&b.id))
                    .map(block_info)
                    .collect();
                for chunk in infos.chunks(900) {
                    broadcast(server, None, &Packet::Blocks { list: chunk.to_vec() });
                }
            }
            SimEvent::BlocksRemoved(ids) => {
                for chunk in ids.chunks(4000) {
                    broadcast(server, None, &Packet::BlockRemove { ids: chunk.to_vec() });
                }
            }
            SimEvent::BodyChanged(id) => {
                if let Some(body) = server.sim.body(id) {
                    let packets = body_def_packets(body);
                    for (&cid, c) in server.clients.iter_mut() {
                        let _ = cid;
                        if c.joined {
                            for p in &packets {
                                c.queue(p);
                            }
                        }
                    }
                }
            }
            SimEvent::BodyRemoved(id) => {
                broadcast(server, None, &Packet::BodyRemove { id });
                // Le corps n'existe plus : son entrée dans la table des
                // véhicules devient inutile (sinon l'emplacement restait
                // définitivement « hors d'usage »).
                server.car_bodies.retain(|_, b| *b != id);
            }
            SimEvent::WorldRemoved(cells) => broadcast_removed(server, &cells),
            SimEvent::WorldPlaced(edits) => broadcast_placed(server, &edits),
            SimEvent::Impact { at, power } => {
                broadcast(server, None, &Packet::Impact { at, power });
            }
        }
    }
}

// ----------------------------------------------------------------------
// Feu
// ----------------------------------------------------------------------

fn fire_rand(server: &mut ServerState) -> f32 {
    let mut x = server.fire_rng;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    server.fire_rng = x;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// Une explosion enflamme les voxels inflammables proches.
fn ignite_around(server: &mut ServerState, center: Vec3, radius: f32) {
    const MAX_FIRES: usize = 3500;
    let r = radius * 1.5 + 0.8;
    let c0 = server.world.world_to_cell(center - Vec3::splat(r));
    let c1 = server.world.world_to_cell(center + Vec3::splat(r));
    let mut added: Vec<[u16; 3]> = Vec::new();
    'scan: for x in c0.x..=c1.x {
        for y in c0.y..=c1.y {
            for z in c0.z..=c1.z {
                let c = IVec3::new(x, y, z);
                if !server.world.is_solid(c) || server.fire.contains_key(&c) {
                    continue;
                }
                let m = server.world.material(c);
                if !crate::world::flammable(m) {
                    continue;
                }
                if server.world.cell_center(c).distance(center) > r || fire_rand(server) > 0.55 {
                    continue;
                }
                if server.fire.len() >= MAX_FIRES {
                    break 'scan;
                }
                let t = crate::world::burn_time(m) * (0.7 + fire_rand(server) * 0.6);
                server.fire.insert(c, t);
                added.push([c.x as u16, c.y as u16, c.z as u16]);
            }
        }
    }
    if !added.is_empty() {
        for chunk in added.chunks(4000) {
            broadcast(server, None, &Packet::Fire { add: chunk.to_vec(), remove: Vec::new() });
        }
    }
}

/// Éteint les flammes autour d'un point (extincteur).
fn douse_around(server: &mut ServerState, center: Vec3, radius: f32) {
    let removed: Vec<IVec3> = server
        .fire
        .keys()
        .copied()
        .filter(|c| server.world.cell_center(*c).distance(center) <= radius)
        .collect();
    if removed.is_empty() {
        return;
    }
    for c in &removed {
        server.fire.remove(c);
    }
    let list: Vec<[u16; 3]> = removed.iter().map(|c| [c.x as u16, c.y as u16, c.z as u16]).collect();
    broadcast(server, None, &Packet::Fire { add: Vec::new(), remove: list });
}

/// Avance le feu : propagation, voxels consumés, brûlures.
fn fire_tick(server: &mut ServerState, dt: f32) {
    if server.fire.is_empty() {
        return;
    }
    const NEIGHBORS: [IVec3; 6] = [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z];
    let cells: Vec<IVec3> = server.fire.keys().copied().collect();
    let mut added: Vec<[u16; 3]> = Vec::new();
    let mut gone: Vec<IVec3> = Vec::new();
    let mut burnt: Vec<IVec3> = Vec::new();
    for c in cells {
        // Un voxel détruit entre-temps ne brûle plus.
        if !server.world.is_solid(c) {
            server.fire.remove(&c);
            gone.push(c);
            continue;
        }
        // Propagation : le feu monte plus volontiers qu'il ne descend.
        if fire_rand(server) < 0.5 * dt * 2.0 {
            let o = NEIGHBORS[(fire_rand(server) * 6.0) as usize % 6];
            let up = if o.y > 0 { 1.0 } else { 0.5 };
            let n = c + o;
            if server.world.is_solid(n)
                && !server.fire.contains_key(&n)
                && server.fire.len() < 3500
                && crate::world::flammable(server.world.material(n))
                && fire_rand(server) < 0.6 * up
            {
                let m = server.world.material(n);
                let t = crate::world::burn_time(m) * (0.7 + fire_rand(server) * 0.6);
                server.fire.insert(n, t);
                added.push([n.x as u16, n.y as u16, n.z as u16]);
            }
        }
        if let Some(t) = server.fire.get_mut(&c) {
            *t -= dt;
            if *t <= 0.0 {
                burnt.push(c);
            }
        }
    }
    for c in &burnt {
        server.fire.remove(c);
        server.world.set(*c, false, 0);
    }
    let mut removed: Vec<[u16; 3]> = gone.iter().map(|c| [c.x as u16, c.y as u16, c.z as u16]).collect();
    removed.extend(burnt.iter().map(|c| [c.x as u16, c.y as u16, c.z as u16]));
    if !added.is_empty() || !removed.is_empty() {
        broadcast(server, None, &Packet::Fire { add: added, remove: removed });
    }
    if !burnt.is_empty() {
        // Ce qui brûle peut faire tomber ce qu'il portait.
        let mut all = burnt.clone();
        let comps = if server.config.rules.gravity_collapse {
            server.world.collapse_components(&burnt)
        } else {
            Vec::new()
        };
        for comp in &comps {
            all.extend_from_slice(comp);
        }
        broadcast_removed(server, &all);
        if !comps.is_empty() {
            let mut ev = Vec::new();
            {
                let s = &mut *server;
                s.sim.form_bodies(&s.world, comps, &mut ev);
            }
            server.events.append(&mut ev);
            flush_events(server);
        }
    }
    // Brûlures : à moins de ~1,2 m d'une flamme.
    let victims: Vec<(u16, Vec3)> = server
        .clients
        .iter()
        .filter(|(_, c)| c.joined && c.placed && c.dead_until.is_none())
        .map(|(&id, c)| (id, c.pos + Vec3::Y * 0.8))
        .collect();
    for (id, p) in victims {
        let cell = server.world.world_to_cell(p);
        let mut near = false;
        'n: for dx in -2..=2 {
            for dy in -2..=2 {
                for dz in -2..=2 {
                    if server.fire.contains_key(&(cell + IVec3::new(dx, dy, dz))) {
                        near = true;
                        break 'n;
                    }
                }
            }
        }
        if near {
            hurt(server, id, None, 9.0 * dt, p, Vec3::ZERO, "brûlé");
        }
    }
}

// ----------------------------------------------------------------------
// Joueurs : dégâts, départ, expulsion, sauvegarde
// ----------------------------------------------------------------------

fn name_of(server: &ServerState, id: u16) -> String {
    server
        .clients
        .get(&id)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| format!("#{id}"))
}

/// Inflige des dégâts à un joueur ; le tue si ses points de vie tombent à 0.
fn hurt(
    server: &mut ServerState,
    victim: u16,
    by: Option<u16>,
    amount: f32,
    from: Vec3,
    push: Vec3,
    cause: &str,
) {
    let rules = server.config.rules.clone();
    if !rules.pvp && by.is_some() && by != Some(victim) {
        return;
    }
    let amount = amount * rules.damage_scale;
    if amount <= 0.0 {
        return;
    }
    let killer = by.map(|b| name_of(server, b));
    let victim_name = name_of(server, victim);
    let Some(c) = server.clients.get_mut(&victim) else {
        return;
    };
    if c.dead_until.is_some() || Instant::now() < c.protected_until {
        return;
    }
    c.hp = (c.hp - amount).max(0.0);
    let hp = c.hp;
    let at = c.pos;
    let dead = hp <= 0.0;
    if dead {
        c.dead_until = Some(Instant::now() + Duration::from_secs_f32(rules.respawn.max(1.0)));
        c.pending.clear();
        c.reload_until = None;
        if !rules.keep_inventory {
            c.inv = fresh_inventory(&rules, server.config.mode);
            c.inv_dirty = true;
        }
    }
    send(server, victim, &Packet::Hurt { hp: hp.ceil() as u8, from });
    if dead {
        let line = match (&killer, by == Some(victim)) {
            (Some(_), true) => format!("{victim_name} s'est tué ({cause})"),
            (Some(k), false) => format!("{victim_name} a été tué par {k} ({cause})"),
            (None, _) => format!("{victim_name} est mort ({cause})"),
        };
        logger::info(&line);
        if server.config.rules.chat && !server.config.solo {
            broadcast(server, None, &Packet::ChatMsg { from: 0, name: String::new(), text: line });
        }
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
        if let Some(c) = server.clients.get_mut(&victim) {
            c.grace_until = Instant::now() + Duration::from_secs(3);
        }
        hurt(
            server,
            victim,
            Some(by),
            amount,
            center,
            away * (6.0 + radius * 2.0) + Vec3::Y * 5.0,
            "explosion",
        );
    }
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

fn max_reach(kind: HitKind) -> f32 {
    match kind {
        HitKind::Sledge => 5.5,
        HitKind::Bullet => 230.0,
        HitKind::Explosion => 0.0,
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

/// Note un comportement suspect ; expulse si le total dépasse le seuil.
///
/// Les états et les tirs arrivent à 20 Hz : sans anti-rebond, un simple à-coup
/// réseau (ou un joueur qui traverse un gravat) notait 20 points par seconde
/// et expulsait un joueur innocent en trois secondes. Un motif n'est donc
/// compté **qu'une fois par fenêtre de [`FLAG_WINDOW`]**, et le compteur se
/// vide avec le temps (voir `serve`).
fn suspect(server: &mut ServerState, id: u16, points: f32, why: &str) {
    let level = server.config.rules.anticheat;
    if level == 0 {
        return;
    }
    let limit = if level >= 2 { 40.0 } else { 80.0 };
    let name = name_of(server, id);
    let now = Instant::now();
    let Some(c) = server.clients.get_mut(&id) else { return };
    // Anti-rebond : une suspicion par fenêtre, quoi qu'il arrive.
    if c.flag_at.is_some_and(|t| now.duration_since(t) < FLAG_WINDOW) {
        return;
    }
    c.flag_at = Some(now);
    c.strikes += points;
    server.rejections += 1;
    if c.strikes > limit {
        logger::warn(format!("{name} expulsé : {why}"));
        kick(server, id, &format!("Expulsé : {why}"));
    } else if c.strikes > limit * 0.5 {
        logger::debug(format!("{name} : suspicion ({why})"));
    }
}

fn kick(server: &mut ServerState, id: u16, reason: &str) {
    if let Some(c) = server.clients.get_mut(&id) {
        c.queue(&Packet::Kicked { reason: reason.to_string() }.encode());
        let _ = c.flush();
    }
    remove_client(server, id, reason);
}

/// Retire un joueur : sauvegarde ses données, informe les autres.
fn remove_client(server: &mut ServerState, id: u16, reason: &str) {
    let Some(client) = server.clients.remove(&id) else { return };
    if !client.joined {
        return;
    }
    logger::info(format!("{} a quitté la partie ({reason})", client.name));
    let save = player_save(&client);
    server.players.insert(client.name.clone(), save);
    if server.host == Some(id) {
        server.host = None;
    }
    broadcast(server, None, &Packet::PlayerLeft { id });
    send_roster(server);
}

fn player_save(c: &Client) -> PlayerSave {
    PlayerSave {
        pos: c.pos,
        hp: c.hp,
        weapon: c.weapon,
        money: c.inv.money,
        earned: c.inv.earned,
        slots: ALL_WEAPONS
            .iter()
            .map(|&w| {
                let s = c.inv.slot(w);
                [s.owned as u32, s.mag, s.reserve]
            })
            .collect(),
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

/// Écrit le monde sur disque.
fn save_world(server: &mut ServerState, name: Option<&str>) -> Result<PathBuf, String> {
    let stem = save::file_stem(name.unwrap_or(&server.config.world_name));
    let path = server.config.save_dir.join(format!("{stem}.bgw"));
    let mut players = server.players.clone();
    for c in server.clients.values().filter(|c| c.joined) {
        players.insert(c.name.clone(), player_save(c));
    }
    let mut sim = W::default();
    server.sim.write(&mut sim);
    let data = SaveData {
        name: stem.clone(),
        saved_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        map_version: crate::map::MAP_VERSION,
        seed: server.world.seed,
        mode: server.config.mode,
        map_blocks: server.config.map_blocks,
        history: server.history.clone(),
        doors_open: server
            .world
            .doors
            .iter()
            .enumerate()
            .filter(|(_, d)| d.is_open)
            .map(|(i, _)| i as u32)
            .collect(),
        sim: sim.0,
        players,
    };
    let started = Instant::now();
    data.write_file(&path)?;
    logger::info(format!(
        "Monde « {stem} » sauvegardé ({} modifications, {} blocs, {} corps) en {} ms",
        data.history.len(),
        server.sim.blocks.len(),
        server.sim.bodies.len(),
        started.elapsed().as_millis()
    ));
    server.last_autosave = Instant::now();
    Ok(path)
}

/// Reconstruit un monde neuf (même seed ou nouvelle).
fn reset_world(server: &mut ServerState, seed: u64) {
    server.world.rebuild(seed);
    server.sim.clear();
    server.history.clear();
    server.events.clear();
    server.fire.clear();
    server.car_bodies.clear();
    for c in server.clients.values_mut() {
        c.driving = None;
    }
    server.config.seed = seed;
    logger::info(format!("Nouvelle ville, seed {seed}"));
    broadcast(server, None, &Packet::Reset { seed });
    let ids: Vec<u16> = server.clients.keys().copied().collect();
    for id in ids {
        let pos = crate::player::random_spawn(&server.world);
        if let Some(c) = server.clients.get_mut(&id) {
            c.pos = pos;
            c.last_valid = pos;
            c.hp = 100.0;
            c.dead_until = None;
            c.placed = false;
        }
        send(server, id, &Packet::Respawned { id, pos });
    }
}

// ----------------------------------------------------------------------
// Boucle principale
// ----------------------------------------------------------------------

fn serve(mut server: ResMut<ServerState>, time: Res<Time>) {
    let now_s = time.elapsed_secs();
    let now = Instant::now();

    // Le compteur de suspicion se vide tout seul (1 point/s) : un joueur
    // signalé puis redevenu normal ne garde pas son marquage.
    let decay = time.delta_secs();
    for c in server.clients.values_mut() {
        c.strikes = (c.strikes - decay).max(0.0);
    }

    // Console du serveur dédié.
    let mut lines = Vec::new();
    if let Some(rx) = &server.console {
        if let Ok(rx) = rx.lock() {
            while let Ok(line) = rx.try_recv() {
                lines.push(line);
            }
        }
    }
    for line in lines {
        console_command(&mut server, &line);
    }

    // Réapparitions.
    let due: Vec<u16> = server
        .clients
        .iter()
        .filter(|(_, c)| c.dead_until.is_some_and(|t| now >= t))
        .map(|(&id, _)| id)
        .collect();
    for id in due {
        let pos = crate::player::random_spawn(&server.world);
        let protect = Duration::from_secs_f32(server.config.rules.spawn_protection);
        if let Some(c) = server.clients.get_mut(&id) {
            c.hp = 100.0;
            c.dead_until = None;
            c.pos = pos;
            c.last_valid = pos;
            c.last_state = now;
            c.protected_until = now + protect;
            c.fall_speed = 0.0;
            c.airborne_since = None;
            c.inv_dirty = true;
        }
        broadcast(&mut server, None, &Packet::Respawned { id, pos });
    }

    // Réponse aux menus du réseau local.
    if !server.config.solo {
        let info = LobbyInfo {
            name: server.config.name.clone(),
            port: server.port,
            players: server.clients.values().filter(|c| c.joined).count() as u8,
            max: server.config.rules.max_players.min(MAX_PLAYERS) as u8,
            mode: server.config.mode,
            started: server.started,
        };
        server.responder.poll(&info);
    }

    // Conducteurs : commandes, position, éjection si le véhicule est HS.
    let drivers: Vec<u16> = server
        .clients
        .iter()
        .filter(|(_, c)| c.driving.is_some())
        .map(|(&id, _)| id)
        .collect();
    for id in drivers {
        let (body_id, drive, fresh, dead) = {
            let c = &server.clients[&id];
            (
                c.driving.unwrap(),
                c.drive,
                now.duration_since(c.drive_at) < Duration::from_millis(400),
                c.dead_until.is_some(),
            )
        };
        let state = server.sim.body_mut(body_id).map(|b| {
            if let Some(v) = b.vehicle.as_mut() {
                if fresh {
                    v.throttle = drive.0 as f32;
                    v.steer = drive.1 as f32;
                    v.brake = drive.2;
                    v.ttl = 0.3;
                }
                (b.pos, v.alive)
            } else {
                (b.pos, false)
            }
        });
        match state {
            Some((pos, alive)) if alive && !dead => {
                if let Some(c) = server.clients.get_mut(&id) {
                    c.pos = pos - Vec3::Y * 0.4;
                    c.last_valid = c.pos;
                    c.placed = true;
                    c.airborne_since = None;
                    c.fall_speed = 0.0;
                }
            }
            Some((_, false)) if !dead => {
                notice(&mut server, id, "Le véhicule est hors d'usage.");
                eject_driver(&mut server, id, true);
            }
            _ => eject_driver(&mut server, id, !dead),
        }
    }

    // Physique : gravats et corps rigides.
    let dt = now.duration_since(server.last_sim).as_secs_f32().min(0.1);
    server.last_sim = now;
    if server.started && !server.paused {
        let mut ev = Vec::new();
        {
            let s = &mut *server;
            s.sim.step(&mut s.world, dt, &mut ev);
        }
        // Morceaux figés dans le monde : s'ils ne tiennent à rien (posés sur
        // un gravat ou un corps parti depuis), ils retombent tout de suite au
        // lieu de flotter jusqu'à la prochaine explosion voisine.
        let placed: Vec<IVec3> = ev
            .iter()
            .filter_map(|e| match e {
                SimEvent::WorldPlaced(cells) => Some(cells),
                _ => None,
            })
            .flatten()
            .map(|c| IVec3::new(c[0] as i32, c[1] as i32, c[2] as i32))
            .collect();
        server.events.append(&mut ev);
        if !placed.is_empty() && server.config.rules.gravity_collapse {
            let comps = server.world.collapse_components(&placed);
            if !comps.is_empty() {
                let all: Vec<IVec3> = comps.iter().flatten().copied().collect();
                broadcast_removed(&mut server, &all);
                let mut ev = Vec::new();
                {
                    let s = &mut *server;
                    s.sim.form_bodies(&s.world, comps, &mut ev);
                }
                server.events.append(&mut ev);
            }
        }
        // Gravats sans appui : vérifiés en continu (4 fois par seconde juste
        // après une destruction, une fois par seconde sinon).
        server.unsupported_for = (server.unsupported_for - dt).max(0.0);
        let every = if server.unsupported_for > 0.0 { 0.25 } else { 1.0 };
        {
            if now.duration_since(server.last_unsupported).as_secs_f32() > every {
                server.last_unsupported = now;
                let s = &mut *server;
                s.sim.wake_unsupported(&s.world);
            }
        }
        flush_events(&mut server);
        crush_and_fall(&mut server, now);
        let since = now.duration_since(server.last_fire).as_secs_f32();
        if since >= 0.25 {
            server.last_fire = now;
            fire_tick(&mut server, since);
        }
    }

    // Recharges terminées.
    let reloaded: Vec<u16> = server
        .clients
        .iter()
        .filter(|(_, c)| c.reload_until.is_some_and(|t| now >= t))
        .map(|(&id, _)| id)
        .collect();
    for id in reloaded {
        if let Some(c) = server.clients.get_mut(&id) {
            c.reload_until = None;
            let w = WeaponId::from_tag(c.weapon);
            let spec = w.spec();
            if spec.kind == Kind::Gun {
                let sandbox = c.inv.sandbox();
                let slot = c.inv.slot_mut(w);
                let need = spec.mag.saturating_sub(slot.mag);
                let take = if sandbox { need } else { need.min(slot.reserve) };
                slot.mag += take;
                if !sandbox {
                    slot.reserve -= take;
                }
                c.inv_dirty = true;
            }
        }
    }

    // Poses des gravats et des corps : 20 fois par seconde.
    if now.duration_since(server.last_pose) > Duration::from_millis(50) {
        server.last_pose = now;
        let blocks: Vec<(u32, Vec3, f32)> = server
            .sim
            .blocks
            .iter()
            .filter(|b| !b.sleeping)
            .map(|b| (b.id, b.pos, b.yaw))
            .collect();
        for chunk in blocks.chunks(1500) {
            broadcast(&mut server, None, &Packet::BlockPose { list: chunk.to_vec() });
        }
        if !server.sim.bodies.is_empty() {
            let list: Vec<(u32, Vec3, [f32; 4])> = server
                .sim
                .bodies
                .iter()
                .map(|b| (b.id, b.pos, b.rot.to_array()))
                .collect();
            broadcast(&mut server, None, &Packet::BodyPose { list });
        }
    }

    // Inventaires modifiés (au plus ~6 fois par seconde et par joueur).
    for c in server.clients.values_mut() {
        if c.inv_dirty && c.joined && now.duration_since(c.last_inv) > Duration::from_millis(160) {
            c.inv_dirty = false;
            c.last_inv = now;
            let data = inventory_packet(&c.inv).encode();
            c.queue(&data);
        }
    }

    // Diffusion des positions, 20 fois par seconde.
    if now_s - server.last_snapshot > 0.05 {
        server.last_snapshot = now_s;
        let ids: Vec<u16> = server.clients.keys().copied().collect();
        for id in ids {
            let snapshot: Vec<Vec<u8>> = server
                .clients
                .iter()
                .filter(|(&other, c)| other != id && c.joined && c.placed && c.dead_until.is_none())
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

    // Lecture des clients.
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
        for frame in frames {
            if let Some(packet) = Packet::decode(&frame) {
                handle(&mut server, id, packet);
            }
        }
        if !alive {
            gone.push(id);
        }
    }

    // Écriture, départs.
    let ids: Vec<u16> = server.clients.keys().copied().collect();
    for id in ids {
        if let Some(c) = server.clients.get_mut(&id) {
            if !c.flush() {
                gone.push(id);
            }
        }
    }
    // Consultations de la liste des serveurs : fermées une fois répondues.
    for (&id, c) in server.clients.iter() {
        if c.close_after && c.outbox.is_empty() {
            gone.push(id);
        }
    }
    gone.sort_unstable();
    gone.dedup();
    for id in gone {
        remove_client(&mut server, id, "connexion fermée");
    }

    // Inactivité.
    let idle = server.config.rules.idle_kick;
    if idle > 0.0 && now.duration_since(server.last_idle_check) > Duration::from_secs(5) {
        server.last_idle_check = now;
        let idle_ids: Vec<u16> = server
            .clients
            .iter()
            .filter(|(_, c)| c.joined && now.duration_since(c.last_activity).as_secs_f32() > idle)
            .map(|(&id, _)| id)
            .collect();
        for id in idle_ids {
            kick(&mut server, id, "inactif trop longtemps");
        }
    }

    // Sauvegarde automatique.
    let every = server.config.rules.autosave_minutes;
    if every > 0.0
        && server.config.dedicated
        && now.duration_since(server.last_autosave).as_secs_f32() > every * 60.0
    {
        if let Err(e) = save_world(&mut server, None) {
            logger::error(format!("Sauvegarde automatique impossible : {e}"));
            server.last_autosave = now;
        }
    }
}

/// Écrasement par les corps en chute et dégâts de chute.
fn crush_and_fall(server: &mut ServerState, now: Instant) {
    if server.sim.blocks.is_empty() && server.sim.bodies.is_empty() {
        return;
    }
    let victims: Vec<(u16, Vec3, Option<u32>)> = server
        .clients
        .iter()
        .filter(|(_, c)| {
            c.joined && c.placed && c.dead_until.is_none() && now.duration_since(c.last_crush) > Duration::from_millis(400)
        })
        .map(|(&id, c)| (id, c.pos, c.driving))
        .collect();
    for (id, pos, driving) in victims {
        let lo = pos + Vec3::new(-0.3, 0.0, -0.3);
        let hi = pos + Vec3::new(0.3, 1.7, 0.3);
        // `driving` est exclu : le conducteur est **dans** son véhicule, il
        // serait sinon écrasé par sa propre voiture dès la première
        // accélération.
        if let Some((mass, speed)) = server.sim.crush_at_except(lo, hi, driving) {
            let amount = (mass * speed * 1.6).min(95.0);
            if amount > 4.0 {
                if let Some(c) = server.clients.get_mut(&id) {
                    c.last_crush = now;
                }
                hurt(server, id, None, amount, pos + Vec3::Y * 3.0, Vec3::Y * 3.0, "écrasé");
            }
        }
    }
}

fn console_command(server: &mut ServerState, line: &str) {
    let mut parts = line.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("").to_lowercase();
    let arg = parts.next().unwrap_or("").trim();
    match cmd.as_str() {
        "" => {}
        "help" | "?" => {
            logger::info("Commandes : help, list, say <texte>, kick <joueur>, ban <joueur>, pardon <joueur>,");
            logger::info("            save [nom], reset [seed], seed, stop");
        }
        "list" => {
            let names: Vec<String> = server
                .clients
                .values()
                .filter(|c| c.joined)
                .map(|c| c.name.clone())
                .collect();
            logger::info(format!("{} joueur(s) : {}", names.len(), names.join(", ")));
        }
        "say" => {
            logger::info(format!("[Serveur] {arg}"));
            broadcast(server, None, &Packet::Notice { text: format!("[Serveur] {arg}") });
        }
        "kick" | "ban" => {
            let target = server
                .clients
                .iter()
                .find(|(_, c)| c.joined && c.name.eq_ignore_ascii_case(arg))
                .map(|(&id, _)| id);
            if cmd == "ban" && !arg.is_empty() {
                let lower = arg.to_lowercase();
                if !server.banned.contains(&lower) {
                    server.banned.push(lower);
                    let _ = std::fs::write(
                        conf::base_dir().join("banned-players.txt"),
                        server.banned.join("\n"),
                    );
                }
                logger::info(format!("{arg} est banni"));
            }
            match target {
                Some(id) => kick(server, id, if cmd == "ban" { "Banni" } else { "Expulsé par le serveur" }),
                None if cmd == "kick" => logger::warn(format!("joueur « {arg} » introuvable")),
                None => {}
            }
        }
        "mute" => {
            let lower = arg.to_lowercase();
            if !lower.is_empty() && !server.muted.contains(&lower) {
                server.muted.push(lower);
            }
            logger::info(format!("{arg} est réduit au silence"));
        }
        "unmute" => {
            let lower = arg.to_lowercase();
            server.muted.retain(|m| *m != lower);
            logger::info(format!("{arg} peut de nouveau parler"));
        }
        "pardon" => {
            let lower = arg.to_lowercase();
            server.banned.retain(|b| *b != lower);
            let _ = std::fs::write(conf::base_dir().join("banned-players.txt"), server.banned.join("\n"));
            logger::info(format!("{arg} n'est plus banni"));
        }
        "save" | "save-all" => {
            let name = (!arg.is_empty()).then_some(arg);
            if let Err(e) = save_world(server, name) {
                logger::error(format!("Sauvegarde impossible : {e}"));
            }
        }
        "reset" => {
            let seed = crate::game::parse_seed(arg).unwrap_or_else(random_seed);
            reset_world(server, seed);
        }
        "seed" => logger::info(format!("Seed : {}", server.world.seed)),
        "stop" | "exit" | "quit" => {
            logger::info("Arrêt du serveur…");
            if server.config.dedicated {
                if let Err(e) = save_world(server, None) {
                    logger::error(format!("Sauvegarde impossible : {e}"));
                }
            }
            let ids: Vec<u16> = server.clients.keys().copied().collect();
            for id in ids {
                kick(server, id, "Serveur arrêté");
            }
            server.shutdown.store(true, Ordering::SeqCst);
        }
        other => logger::warn(format!("commande inconnue : {other} (tape « help »)")),
    }
}

/// Le joueur est-il posé sur quelque chose ?
fn standing(world: &VoxelWorld, pos: Vec3) -> bool {
    world.overlaps(
        Vec3::new(pos.x - 0.25, pos.y - 0.15, pos.z - 0.25),
        Vec3::new(pos.x + 0.25, pos.y + 0.03, pos.z + 0.25),
    )
}

/// Rayon maximal accepté pour un coup (arme en main).
fn blast_cap(kind: HitKind, weapon: WeaponId, rules: &Rules) -> f32 {
    let r = weapon.spec().radius;
    match kind {
        HitKind::Explosion => r * rules.explosion_scale * 1.05,
        _ => r * 1.1,
    }
}

fn handle(server: &mut ServerState, id: u16, packet: Packet) {
    match packet {
        Packet::Join { name, token, version } => handle_join(server, id, name, token, version),
        Packet::Start => {
            if server.host == Some(id) && !server.started {
                server.started = true;
                logger::info("La partie commence");
                broadcast(server, None, &Packet::Start);
            }
        }
        Packet::State { pos, yaw, pitch, weapon } => handle_state(server, id, pos, yaw, pitch, weapon),
        Packet::Hit { kind, origin, dir, blast, shot } => handle_hit(server, id, kind, origin, dir, blast, shot),
        Packet::Place { cell, material } => {
            if !server.started || server.paused {
                return;
            }
            let rules = server.config.rules.clone();
            let normal = server.config.mode == 1;
            let Some(client) = server.clients.get_mut(&id) else { return };
            if !client.placed || !client.joined || client.dead_until.is_some() || !take_token(client, 1.0) {
                server.rejections += 1;
                return;
            }
            if !rules.allow_building {
                return;
            }
            let at = IVec3::new(cell[0] as i32, cell[1] as i32, cell[2] as i32);
            let center = server.world.cell_center(at);
            let near = center.distance(client.pos + Vec3::Y * 1.6) <= 12.0;
            let d = server.world.dims;
            let inside = at.y >= 1 && at.cmpge(IVec3::ZERO).all() && at.cmplt(d).all();
            // Pas de bloc dans un joueur.
            let in_player = server.clients.values().any(|c| {
                c.joined && {
                    let lo = c.pos + Vec3::new(-0.3, 0.0, -0.3);
                    let hi = c.pos + Vec3::new(0.3, 1.8, 0.3);
                    let cl = server.world.cell_min(at);
                    cl.cmplt(hi).all() && (cl + Vec3::splat(crate::world::VOXEL)).cmpgt(lo).all()
                }
            });
            let Some(client) = server.clients.get_mut(&id) else { return };
            if !near
                || !inside
                || in_player
                || server.world.is_solid(at)
                || material as usize >= crate::world::PALETTE.len()
            {
                server.rejections += 1;
                return;
            }
            if normal {
                if client.inv.money < 1 {
                    return;
                }
                client.inv.money -= 1;
                client.inv_dirty = true;
            }
            client.last_activity = Instant::now();
            server.world.set(at, true, material);
            let edit = [cell[0], cell[1], cell[2], material as u16];
            broadcast_placed(server, &[edit]);
        }
        Packet::Interact { door } => {
            if !server.started || server.paused || !server.config.rules.allow_doors {
                return;
            }
            let Some(client) = server.clients.get_mut(&id) else { return };
            if !client.placed || !client.joined || client.dead_until.is_some() || !take_token(client, 2.0) {
                return;
            }
            client.last_activity = Instant::now();
            let pos = client.pos + Vec3::Y * 1.6;
            let index = door as usize;
            let Some(d) = server.world.doors.get(index) else { return };
            if d.center(&server.world).distance(pos) > 7.0 {
                suspect(server, id, 2.0, "porte hors de portée");
                return;
            }
            if let Some(edits) = server.world.toggle_door(index) {
                let open = server.world.doors[index].is_open;
                broadcast_placed(server, &edits);
                broadcast(server, None, &Packet::DoorState { door, open });
            } else {
                notice(server, id, "La porte est cassée ou bloquée.");
            }
        }
        Packet::BombPlaced { at, weapon } => {
            if !server.started || server.paused {
                return;
            }
            let rules = server.config.rules.clone();
            let w = WeaponId::from_tag(weapon);
            let Some(client) = server.clients.get_mut(&id) else { return };
            if !client.joined || client.dead_until.is_some() || w.kind() != Kind::Placed || !rules.weapon_enabled(w) {
                return;
            }
            if at.distance(client.pos) > 12.0 || !client.inv.use_bomb(w) || client.pending.len() >= 40 {
                server.rejections += 1;
                return;
            }
            client.inv_dirty = true;
            client.last_activity = Instant::now();
            client.pending.push((w.spec().radius * rules.explosion_scale * 1.05, Instant::now(), w));
            broadcast(server, Some(id), &Packet::BombPlaced { at, weapon });
        }
        Packet::Throw { weapon } => {
            if !server.started || server.paused {
                return;
            }
            let rules = server.config.rules.clone();
            let w = WeaponId::from_tag(weapon);
            let Some(client) = server.clients.get_mut(&id) else { return };
            if !client.joined || client.dead_until.is_some() || !rules.weapon_enabled(w) {
                return;
            }
            let ok = match w.kind() {
                Kind::Thrown => client.inv.use_bomb(w),
                Kind::Gun if w.spec().projectile || w.spec().fuse > 0.0 => {
                    let slot = client.inv.slot_mut(w);
                    if slot.mag > 0 && client.reload_until.is_none() {
                        slot.mag -= 1;
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            };
            if !ok || client.pending.len() >= 40 {
                server.rejections += 1;
                return;
            }
            client.inv_dirty = true;
            client.last_activity = Instant::now();
            client.pending.push((w.spec().radius * rules.explosion_scale * 1.05, Instant::now(), w));
        }
        Packet::Reload => {
            let Some(client) = server.clients.get_mut(&id) else { return };
            let w = WeaponId::from_tag(client.weapon);
            let spec = w.spec();
            if spec.kind != Kind::Gun || client.reload_until.is_some() || client.dead_until.is_some() {
                return;
            }
            let slot = *client.inv.slot(w);
            let can = slot.mag < spec.mag && (client.inv.sandbox() || slot.reserve > 0);
            if can {
                client.reload_until = Some(Instant::now() + Duration::from_secs_f32(spec.reload * 0.9));
                client.last_activity = Instant::now();
            }
        }
        Packet::Buy { weapon, ammo } => {
            let rules = server.config.rules.clone();
            let w = WeaponId::from_tag(weapon);
            let Some(client) = server.clients.get_mut(&id) else { return };
            if !client.joined || client.inv.sandbox() {
                return;
            }
            if !rules.weapon_enabled(w) || (w == WeaponId::Blocks && !rules.allow_building) {
                let text = "Cette arme est désactivée sur ce serveur.";
                send(server, id, &Packet::Notice { text: text.into() });
                return;
            }
            let result = if ammo { client.inv.buy_ammo(w) } else { client.inv.buy(w) };
            client.inv_dirty = true;
            client.last_activity = Instant::now();
            if let Err(e) = result {
                send(server, id, &Packet::Notice { text: e.into() });
            }
        }
        Packet::Command { text } => handle_command(server, id, &text),
        Packet::Status => {
            let r = &server.config.rules;
            let info = Packet::StatusInfo {
                name: server.config.name.clone(),
                players: server.clients.values().filter(|c| c.joined).count().min(255) as u8,
                max: r.max_players.min(MAX_PLAYERS) as u8,
                mode: server.config.mode,
                version: PROTOCOL_VERSION,
                motd: server.config.motd.clone(),
            };
            if let Some(c) = server.clients.get_mut(&id) {
                if !c.joined {
                    c.queue(&info.encode());
                    c.close_after = true;
                }
            }
        }
        Packet::Chat { text } => handle_chat(server, id, text),
        Packet::Enter { car } => handle_enter(server, id, car),
        Packet::Exit => eject_driver(server, id, true),
        Packet::Drive { throttle, steer, brake } => {
            if let Some(c) = server.clients.get_mut(&id) {
                c.drive = (throttle.clamp(-1, 1), steer.clamp(-1, 1), brake);
                c.drive_at = Instant::now();
                c.last_activity = c.drive_at;
            }
        }
        Packet::Voice { seq, data, .. } => handle_voice(server, id, seq, data),
        Packet::Ping => {
            if let Some(c) = server.clients.get_mut(&id) {
                c.last_activity = Instant::now();
            }
        }
        _ => {}
    }
}

/// Remplace les mots interdits par des étoiles (sans tenir compte de la
/// casse).
fn filter_text(text: &str, words: &[String]) -> String {
    let mut out = text.to_string();
    for w in words {
        let mut lower = out.to_lowercase();
        // Les accents ne changent pas la longueur en octets pour ces
        // comparaisons simples : on se limite aux correspondances sûres.
        while let Some(i) = lower.find(w.as_str()) {
            if out.is_char_boundary(i) && out.is_char_boundary(i + w.len()) && out.len() == lower.len() {
                out.replace_range(i..i + w.len(), &"*".repeat(w.chars().count()));
                lower.replace_range(i..i + w.len(), &"*".repeat(w.chars().count()));
            } else {
                break;
            }
        }
    }
    out
}

/// Monte dans une voiture : la première fois, ses voxels deviennent un
/// corps rigide conduisible.
fn handle_enter(server: &mut ServerState, id: u16, car: u16) {
    if !server.started || server.paused {
        return;
    }
    let Some(c) = server.clients.get(&id) else { return };
    if !c.joined || !c.placed || c.dead_until.is_some() || c.driving.is_some() {
        return;
    }
    let player = c.pos;
    let Some(&min) = server.world.cars.get(car as usize) else { return };
    let body_id = match server.car_bodies.get(&car).copied() {
        Some(b) if server.sim.body(b).is_some() => {
            // La voiture roule déjà : c'est sa position **actuelle** qui
            // compte, pas sa case d'origine dans la carte (elle a pu être
            // garée ailleurs après le premier voyage).
            let at = server.sim.body(b).map(|body| body.pos).unwrap_or(Vec3::ZERO);
            if at.distance(player + Vec3::Y) > 7.0 {
                notice(server, id, "Ce véhicule est trop loin.");
                return;
            }
            b
        }
        Some(_) => {
            // Le corps a disparu (voiture tombée hors du monde) : on oublie
            // cette référence au lieu de condamner définitivement le
            // emplacement.
            server.car_bodies.remove(&car);
            notice(server, id, "Ce véhicule est hors d'usage.");
            return;
        }
        None => {
            let center = server.world.cell_center(min + IVec3::new(2, 3, 5));
            if center.distance(player + Vec3::Y) > 7.0 {
                suspect(server, id, 1.0, "véhicule hors de portée");
                return;
            }
            let mut cells = Vec::new();
            for x in 0..5 {
                for y in 0..6 {
                    for z in 0..11 {
                        let cell = min + IVec3::new(x, y, z);
                        if server.world.is_solid(cell) {
                            cells.push(cell);
                        }
                    }
                }
            }
            if cells.len() < 60 {
                notice(server, id, "Cette voiture est trop abîmée.");
                return;
            }
            for &cell in &cells {
                server.world.set(cell, false, 0);
            }
            broadcast_removed(server, &cells);
            let made = {
                let s = &mut *server;
                s.sim.form_vehicle(&s.world, cells)
            };
            let Some(b) = made else {
                notice(server, id, "Impossible de démarrer ce véhicule.");
                return;
            };
            server.car_bodies.insert(car, b);
            server.events.push(SimEvent::BodyChanged(b));
            flush_events(server);
            b
        }
    };
    // Place prise ?
    if server.clients.values().any(|c| c.driving == Some(body_id)) {
        notice(server, id, "La place est prise.");
        return;
    }
    let alive = server.sim.body(body_id).and_then(|b| b.vehicle.as_ref()).is_some_and(|v| v.alive);
    if !alive {
        notice(server, id, "Le véhicule est trop endommagé pour rouler.");
        return;
    }
    if let Some(c) = server.clients.get_mut(&id) {
        c.driving = Some(body_id);
        c.drive = (0, 0, false);
        c.drive_at = Instant::now();
    }
    send(server, id, &Packet::Driving { body: body_id });
}

/// Fait descendre un conducteur (à côté du véhicule si `place`).
fn eject_driver(server: &mut ServerState, id: u16, place: bool) {
    let Some(c) = server.clients.get_mut(&id) else { return };
    let Some(body_id) = c.driving.take() else { return };
    let mut target = None;
    if place {
        if let Some(b) = server.sim.body(body_id) {
            let fwd = b.rot * Vec3::NEG_Z;
            let side = Vec3::new(-fwd.z, 0.0, fwd.x).normalize_or_zero();
            // On cherche le sol **au niveau du véhicule**, jamais plus haut :
            // c'était ce qui posait le joueur sur le toit de l'immeuble le
            // plus proche quand il sortait d'une voiture garée à côté.
            let ceiling = b.pos.y + 1.2;
            for s in [3.0, -3.0, 4.5, 0.0, -4.5, 6.0] {
                let p = b.pos + side * s;
                let ground = server.world.ground_height_below(p.x, p.z, ceiling);
                // Un sol bien plus bas que la voiture = un trou ou une falaise.
                if ground < b.pos.y - 6.0 {
                    continue;
                }
                let candidate = Vec3::new(p.x, ground + 0.1, p.z);
                if !server.world.overlaps(candidate + Vec3::new(-0.3, 0.1, -0.3), candidate + Vec3::new(0.3, 1.7, 0.3)) {
                    target = Some(candidate);
                    break;
                }
            }
            if target.is_none() {
                // Dernier recours : à côté du véhicule, un peu plus bas.
                let p = b.pos - Vec3::Y * 0.8;
                target = Some(p);
            }
        }
    }
    if let Some(c) = server.clients.get_mut(&id) {
        if let Some(p) = target {
            c.pos = p;
            c.last_valid = p;
            c.teleporting = Some((p, Instant::now()));
            c.queue(&Packet::Teleport { pos: p }.encode());
        }
        c.queue(&Packet::Driving { body: 0 }.encode());
    }
}

/// Un tir a touché un véhicule : on l'écaille, on réveille ce qui traîne
/// autour et on prévient le tireur si la voiture devient inutilisable.
fn damage_vehicle(server: &mut ServerState, id: u16, body: u32, point: Vec3, blast: f32, kind: HitKind) {
    let mut ev = Vec::new();
    {
        let s = &mut *server;
        s.sim.damage_bodies(point, blast.max(0.3), &mut ev);
        s.sim.wake_near(point, blast.max(0.3), if kind == HitKind::Explosion { 4.0 } else { 0.0 });
    }
    // La voiture a pu être totalement détruite : on oublie sa position dans
    // la table des véhicules plutôt que de la laisser morte à jamais.
    let gone = server.sim.body(body).is_none();
    if gone {
        server.car_bodies.retain(|_, b| *b != body);
    }
    if let Some(c) = server.clients.get_mut(&id) {
        c.inv.reward(6);
        c.inv_dirty = true;
    }
    server.events.append(&mut ev);
    server.unsupported_for = 3.0;
    flush_events(server);
    if gone {
        // Les personnes à bord sont éjectées par la boucle de conduite.
        notice(server, id, "Véhicule détruit.");
    }
}

/// Chat : nettoyage, limite de débit, anti-répétition, mots interdits.
fn handle_chat(server: &mut ServerState, id: u16, text: String) {
    let rules = server.config.rules.clone();
    if !rules.chat || server.config.solo {
        return;
    }
    let name = name_of(server, id);
    let muted = server.muted.contains(&name.to_lowercase());
    let Some(c) = server.clients.get_mut(&id) else { return };
    if !c.joined {
        return;
    }
    let now = Instant::now();
    let clean: String = text
        .chars()
        .filter(|ch| !ch.is_control())
        .take(200)
        .collect::<String>()
        .trim()
        .to_string();
    if clean.is_empty() {
        return;
    }
    c.last_activity = now;
    if muted {
        c.queue(&Packet::ChatMsg { from: 0, name: String::new(), text: "Tu es réduit au silence.".into() }.encode());
        return;
    }
    // Débit : `chat_burst` messages par fenêtre de 8 s.
    while c.chat_times.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(8)) {
        c.chat_times.pop_front();
    }
    if c.chat_times.len() as u32 >= rules.chat_burst {
        c.queue(&Packet::ChatMsg { from: 0, name: String::new(), text: "Doucement ! Trop de messages.".into() }.encode());
        return;
    }
    // Pas deux fois le même message en moins de 4 s.
    if clean.eq_ignore_ascii_case(&c.last_chat) && now.duration_since(c.last_chat_at) < Duration::from_secs(4) {
        return;
    }
    c.chat_times.push_back(now);
    c.last_chat = clean.clone();
    c.last_chat_at = now;
    let shown = filter_text(&clean, &rules.chat_filter);
    if shown != clean {
        logger::debug(format!("{name} : message filtré"));
    }
    logger::info(format!("<{name}> {shown}"));
    broadcast(server, None, &Packet::ChatMsg { from: id, name, text: shown });
}

/// Voix de proximité : relayée aux joueurs à portée.
fn handle_voice(server: &mut ServerState, id: u16, seq: u16, data: Vec<u8>) {
    let rules = server.config.rules.clone();
    if !rules.voice || server.config.solo || data.is_empty() || data.len() > 700 {
        return;
    }
    let name = name_of(server, id);
    let muted = server.muted.contains(&name.to_lowercase());
    let now = Instant::now();
    let Some(c) = server.clients.get_mut(&id) else { return };
    if !c.joined || !c.placed || c.dead_until.is_some() || muted {
        return;
    }
    // Seau de jetons : ~60 trames par seconde au maximum.
    let dt = now.duration_since(c.voice_refill).as_secs_f32();
    c.voice_refill = now;
    c.voice_tokens = (c.voice_tokens + dt * 60.0).min(100.0);
    if c.voice_tokens < 1.0 {
        return;
    }
    c.voice_tokens -= 1.0;
    c.last_activity = now;
    let (from_pos, range) = (c.pos, rules.voice_range);
    let packet = Packet::Voice { from: id, seq, data }.encode();
    for (&other, client) in server.clients.iter_mut() {
        if other != id && client.joined && client.dead_until.is_none() && client.pos.distance(from_pos) <= range {
            client.queue(&packet);
        }
    }
}

fn handle_join(server: &mut ServerState, id: u16, name: String, token: u64, version: u16) {
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
        logger::warn(format!("#{id} refusé : version {version} (serveur : {PROTOCOL_VERSION})"));
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
    if server.banned.contains(&base.to_lowercase()) {
        send(server, id, &Packet::Refused { reason: "tu es banni de ce serveur".into() });
        logger::warn(format!("{base} refusé : banni"));
        return;
    }
    let mut unique = base.clone();
    let mut n = 2;
    while server.clients.values().any(|c| c.joined && c.name == unique) {
        unique = format!("{base} ({n})");
        n += 1;
    }
    let host_join = (server.config.solo || !server.config.dedicated)
        && token == server.config.token
        && server.host.is_none();
    if host_join {
        server.host = Some(id);
    }

    let r = &server.config.rules;
    let flags = (r.allow_fly || server.config.solo) as u8
        | ((r.chat && !server.config.solo) as u8) << 1
        | ((r.voice && !server.config.solo) as u8) << 2;
    let welcome = Packet::Welcome {
        id,
        seed: server.world.seed,
        mode: server.config.mode,
        started: server.started,
        map_blocks: server.config.map_blocks,
        name: server.config.name.clone(),
        max_players: r.max_players.min(MAX_PLAYERS) as u8,
        flags,
    };
    // État du monde : modifications, gravats, corps, portes.
    let mut stream: Vec<Vec<u8>> = vec![welcome.encode()];
    stream.extend(encode_history(&server.history));
    let infos: Vec<BlockInfo> = server.sim.blocks.iter().map(block_info).collect();
    for chunk in infos.chunks(900) {
        stream.push(Packet::Blocks { list: chunk.to_vec() }.encode());
    }
    for body in &server.sim.bodies {
        stream.extend(body_def_packets(body));
    }
    for (i, d) in server.world.doors.iter().enumerate() {
        if d.is_open {
            stream.push(Packet::DoorState { door: i as u16, open: true }.encode());
        }
    }
    let fires: Vec<[u16; 3]> = server.fire.keys().map(|c| [c.x as u16, c.y as u16, c.z as u16]).collect();
    for chunk in fires.chunks(4000) {
        stream.push(Packet::Fire { add: chunk.to_vec(), remove: Vec::new() }.encode());
    }

    // Données d'un joueur déjà connu (même nom).
    let saved = server.players.get(&unique).cloned();
    let rules = server.config.rules.clone();
    let mode = server.config.mode;
    let spawn = crate::player::random_spawn(&server.world);
    if let Some(c) = server.clients.get_mut(&id) {
        c.name = unique.clone();
        c.joined = true;
        c.last_activity = Instant::now();
        c.inv = fresh_inventory(&rules, mode);
        if let Some(s) = &saved {
            c.hp = s.hp.clamp(1.0, 100.0);
            c.inv.money = s.money;
            c.inv.earned = s.earned;
            for (i, w) in ALL_WEAPONS.iter().enumerate() {
                if let Some(v) = s.slots.get(i) {
                    let slot = c.inv.slot_mut(*w);
                    slot.owned = v[0] != 0 || slot.owned;
                    slot.mag = v[1];
                    slot.reserve = v[2];
                }
            }
            apply_restrictions(&mut c.inv, &rules);
            c.pos = s.pos;
            c.last_valid = s.pos;
            c.placed = false;
        } else {
            c.pos = spawn;
            c.last_valid = spawn;
        }
        c.inv_dirty = true;
        for data in &stream {
            c.queue(data);
        }
        // Le point d'apparition est toujours imposé par le serveur : le
        // client l'ignore et se téléporte. Sans ce paquet un nouveau joueur
        // restait sur son spawn local (le carrefour) au lieu d'apparaître
        // sur le trottoir choisi par le serveur.
        c.queue(&Packet::Teleport { pos: c.pos }.encode());
        c.teleporting = Some((c.pos, Instant::now()));
        c.placed = false;
        c.rejected = 0;
    }
    let addr = server.clients.get(&id).map(|c| c.addr.to_string()).unwrap_or_default();
    logger::info(format!("{unique} a rejoint la partie (#{id}, {addr})"));
    send_roster(server);
}

fn handle_state(server: &mut ServerState, id: u16, pos: Vec3, yaw: f32, pitch: f32, weapon: u8) {
    if !pos.is_finite() || !yaw.is_finite() || !pitch.is_finite() {
        suspect(server, id, 20.0, "position invalide");
        return;
    }
    let level = server.config.rules.anticheat;
    let sandbox = server.config.mode == 0;
    // Vol libre : bac à sable ET autorisé par les règles (ou solo).
    let fly_ok = sandbox && (server.config.rules.allow_fly || server.config.solo);
    let now = Instant::now();
    let in_bounds = {
        let d = server.world.dims;
        let o = server.world.origin;
        let size = d.as_vec3() * crate::world::VOXEL;
        pos.x > o.x - 2.0 && pos.x < o.x + size.x + 2.0 && pos.z > o.z - 2.0 && pos.z < o.z + size.z + 2.0 && pos.y > 0.3 && pos.y < 120.0
    };
    let Some(c) = server.clients.get_mut(&id) else { return };
    if c.dead_until.is_some() || !c.joined {
        return;
    }
    // Au volant : la position vient du véhicule.
    if c.driving.is_some() {
        c.yaw = yaw;
        c.pitch = pitch;
        return;
    }
    // Téléportation en cours (chargement, correction) : on attend que le
    // client y soit arrivé.
    if let Some((target, since)) = c.teleporting {
        if pos.distance(target) < 4.0 || since.elapsed() > Duration::from_secs(4) {
            c.teleporting = None;
            c.placed = false;
        } else {
            return;
        }
    }
    c.yaw = yaw;
    c.pitch = pitch;
    if WeaponId::from_tag(weapon).tag() == weapon {
        if c.weapon != weapon {
            c.reload_until = None;
        }
        c.weapon = weapon;
    }
    if !in_bounds {
        let back = c.last_valid;
        c.teleporting = Some((back, now));
        c.queue(&Packet::Teleport { pos: back }.encode());
        return;
    }
    let dt = now.duration_since(c.last_state).as_secs_f32().clamp(0.005, 1.0);
    if !c.placed {
        c.pos = pos;
        c.last_valid = pos;
        c.placed = true;
        c.last_state = now;
        c.rejected = 0;
        return;
    }
    if pos.distance(c.pos) > 0.04 {
        c.last_activity = now;
    }
    let grace = now < c.grace_until;
    let strict = level >= 2;
    // Les états arrivent en rafales (TCP) : on ne compte jamais moins de
    // 100 ms, sinon un vol rapide serait pris pour de la triche.
    let dt = dt.max(0.1);
    let max_h = if grace {
        45.0
    } else if fly_ok {
        34.0
    } else if strict {
        9.5
    } else {
        11.0
    };
    let max_up = if grace {
        30.0
    } else if fly_ok {
        34.0
    } else {
        12.0
    };
    let dx = pos.x - c.pos.x;
    let dz = pos.z - c.pos.z;
    let horizontal = (dx * dx + dz * dz).sqrt();
    let dy = pos.y - c.pos.y;
    let too_fast = level > 0 && (horizontal > max_h * dt * 1.2 + 0.7 || dy > max_up * dt * 1.2 + 0.7);
    // Traversée de mur : la nouvelle boîte ne doit pas être dans du solide.
    let inside = level > 0 && {
        // Boîte nettement plus petite que celle du client : la carte du
        // serveur peut avoir un voxel d'avance ou de retard (explosion,
        // porte, marche montée), et ça ne doit pas passer pour de la triche.
        let lo = pos + Vec3::new(-0.18, 0.7, -0.18);
        let hi = pos + Vec3::new(0.18, 1.4, 0.18);
        server.world.overlaps(lo, hi)
    };
    let Some(c) = server.clients.get_mut(&id) else { return };
    if too_fast || inside {
        c.rejected += 1;
        let rejected = c.rejected;
        if rejected >= 6 {
            let back = c.last_valid;
            c.rejected = 0;
            c.teleporting = Some((back, now));
            c.queue(&Packet::Teleport { pos: back }.encode());
        }
        // On ne note une suspicion qu'au bout de `STRIKE_AFTER` refus
        // consécutifs : un état isolé qui dépasse le seuil (à-coup réseau,
        // gravat qui pousse le joueur) ne doit plus être facturé.
        if rejected == STRIKE_AFTER || rejected % 20 == 0 {
            suspect(server, id, if inside { 2.0 } else { 3.0 }, if inside { "traverse les murs" } else { "déplacement trop rapide" });
        }
        return;
    }
    c.rejected = 0;
    // Chute : vitesse verticale mesurée entre deux états.
    let vy = dy / dt;
    c.fall_speed = if vy < -2.0 { c.fall_speed.min(vy) } else { c.fall_speed };
    let world = &server.world;
    let on_ground = standing(world, pos);
    // Debout sur un véhicule ou des gravats (absents de la grille) : pas du
    // vol. On ne soupçonne qu'un joueur nettement au-dessus du sol.
    let high = pos.y - world.ground_height_below(pos.x, pos.z, pos.y + 0.1) > 3.0;
    let c = server.clients.get_mut(&id).unwrap();
    c.pos = pos;
    c.last_state = now;
    let mut fall_damage = 0.0;
    if on_ground {
        if c.fall_speed < -14.0 {
            fall_damage = (-c.fall_speed - 13.0) * 7.5;
        }
        c.fall_speed = 0.0;
        c.airborne_since = None;
        c.last_valid = pos;
    } else if vy > -1.0 {
        // En l'air sans tomber : saut (court) ou vol. On laisse une marge
        // large : rester sur un gravat, un véhicule ou une porte qui se
        // referme passe ici aussi, sans être de la triche.
        let since = *c.airborne_since.get_or_insert(now);
        if !fly_ok && level > 0 && high && since.elapsed() > Duration::from_secs(6) && now >= c.grace_until {
            c.airborne_since = Some(now);
            suspect(server, id, 4.0, "vol");
            return;
        }
    } else {
        c.airborne_since = None;
    }
    if fall_damage > 0.0 && server.config.rules.fall_damage && !sandbox {
        hurt(server, id, None, fall_damage, pos, Vec3::ZERO, "chute");
    } else if fall_damage > 0.0 && server.config.rules.fall_damage {
        hurt(server, id, None, fall_damage * 0.5, pos, Vec3::ZERO, "chute");
    }
}

fn handle_hit(server: &mut ServerState, id: u16, kind: HitKind, origin: Vec3, dir: Vec3, blast: f32, shot: u16) {
    if !server.started || server.paused {
        return;
    }
    if !origin.is_finite() || !dir.is_finite() || !blast.is_finite() {
        suspect(server, id, 20.0, "tir invalide");
        return;
    }
    let rules = server.config.rules.clone();
    let now = Instant::now();
    let Some(client) = server.clients.get_mut(&id) else { return };
    let cost = if kind == HitKind::Explosion { 4.0 } else { 1.0 };
    if !client.placed || !client.joined || client.dead_until.is_some() || !take_token(client, cost) {
        server.rejections += 1;
        return;
    }
    client.last_activity = now;
    let weapon = WeaponId::from_tag(client.weapon);
    let spec = weapon.spec();
    if !rules.weapon_enabled(weapon) {
        if now.duration_since(client.last_notice) > Duration::from_secs(3) {
            client.last_notice = now;
            client.queue(
                &Packet::Notice {
                    text: format!("{} est désactivé sur ce serveur.", spec.name),
                }
                .encode(),
            );
        }
        return;
    }
    let player = client.pos + Vec3::Y * 1.6;
    let mut cap = blast_cap(kind, weapon, &rules);
    // Arme dont vient l'explosion (bombe armée), pour l'incendie.
    let mut exploded: Option<WeaponId> = None;

    let violation = match kind {
        HitKind::Sledge => {
            let gap = (spec.cooldown * 0.6).max(0.04);
            if spec.kind != Kind::Melee {
                Some(("coup de marteau avec une arme à feu", 2.0))
            } else if now.duration_since(client.last_swing_at).as_secs_f32() < gap {
                Some(("cadence de frappe", 0.4))
            } else {
                client.last_swing_at = now;
                None
            }
        }
        HitKind::Bullet => {
            if spec.kind != Kind::Gun || spec.projectile || spec.fuse > 0.0 {
                Some(("tir avec une arme sans munitions", 2.0))
            } else if client.reload_until.is_some() {
                Some(("tir pendant le rechargement", 0.5))
            } else {
                let new_shot = shot != client.last_shot_id || client.pellets_left == 0;
                if new_shot {
                    let gap = spec.cooldown * 0.7;
                    if now.duration_since(client.last_shot_at).as_secs_f32() < gap {
                        Some(("cadence de tir", 0.5))
                    } else if client.inv.slot(weapon).mag == 0 {
                        Some(("tir sans munitions", 1.0))
                    } else {
                        client.inv.slot_mut(weapon).mag -= 1;
                        client.inv_dirty = true;
                        client.last_shot_id = shot;
                        client.pellets_left = spec.pellets.max(1);
                        client.last_shot_at = now;
                        None
                    }
                } else {
                    None
                }
            }
        }
        HitKind::Explosion => {
            // Une explosion doit correspondre à une bombe armée.
            let idx = client
                .pending
                .iter()
                .enumerate()
                .filter(|(_, (c, _, _))| *c >= blast * 0.95)
                .min_by(|a, b| a.1 .0.total_cmp(&b.1 .0))
                .map(|(i, _)| i);
            match idx {
                Some(i) => {
                    cap = client.pending[i].0;
                    exploded = Some(client.pending[i].2);
                    client.pending.remove(i);
                    None
                }
                None => Some(("explosion sans bombe", 3.0)),
            }
        }
    };
    if let Some((why, points)) = violation {
        suspect(server, id, points, why);
        return;
    }
    let Some(client) = server.clients.get_mut(&id) else { return };
    if kind == HitKind::Bullet {
        if client.pellets_left == 0 {
            return;
        }
        client.pellets_left -= 1;
    }
    let blast = blast.clamp(0.05, cap.max(0.05));

    let center = if kind == HitKind::Explosion {
        if origin.distance(player) > MAX_EXPLOSION_DISTANCE {
            suspect(server, id, 3.0, "explosion trop lointaine");
            return;
        }
        origin
    } else {
        // L'origine doit être près du joueur connu, et le rayon doit
        // réellement toucher quelque chose.
        if origin.distance(player) > MAX_ORIGIN_DRIFT {
            suspect(server, id, 1.0, "tir depuis une position incohérente");
            return;
        }
        let reach = max_reach(kind).min(spec.reach.max(1.0) + 1.0);
        let world_hit = server.world.raycast(origin, dir, reach);
        // Les véhicules sont des corps rigides, invisibles pour la grille du
        // monde : sans ce second lancer de rayon, on ne pouvait pas les
        // détruire une fois qu'on y était monté.
        let car_hit = server.sim.raycast(origin, dir, reach);
        // Le plus proche des deux touche.
        let limit = match (world_hit.as_ref(), car_hit.as_ref()) {
            (Some(w), Some(c)) => w.point.distance(origin).min(c.0.distance(origin)),
            (Some(w), None) => w.point.distance(origin),
            (None, Some(c)) => c.0.distance(origin),
            (None, None) => reach,
        };
        // Un joueur sur la trajectoire est blessé.
        let dmg = spec.damage;
        if dmg > 0.0 {
            let dir_n = dir.normalize_or_zero();
            let target = server
                .clients
                .iter()
                .filter(|(&other, c)| other != id && c.joined && c.placed && c.dead_until.is_none())
                .filter_map(|(&other, c)| ray_box(origin, dir_n, c.pos, limit).map(|t| (other, t)))
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((victim, _)) = target {
                let cause = if kind == HitKind::Sledge { "coup" } else { "balle" };
                hurt(server, victim, Some(id), dmg, origin, dir_n * 4.0, cause);
            }
        }
        match world_hit {
            Some(hit) => {
                // Extincteur : éteint, ne détruit rien.
                if spec.extinguish {
                    douse_around(server, hit.point, 2.6);
                    return;
                }
                // Balles incendiaires (fusée, lance-flammes, plasma).
                if spec.incendiary > 0.0 {
                    ignite_around(server, hit.point, spec.incendiary * 0.45);
                }
                hit.point
            }
            None => match car_hit {
                // Impact sur un véhicule : on l'écaille directement.
                Some((point, body)) => {
                    if spec.extinguish {
                        douse_around(server, point, 2.6);
                        return;
                    }
                    if spec.incendiary > 0.0 {
                        ignite_around(server, point, spec.incendiary * 0.45);
                    }
                    damage_vehicle(server, id, body, point, blast, kind);
                    return;
                }
                None => return,
            },
        }
    };

    // Destruction, effondrement, gravats, argent.
    let mut destroyed = server.world.carve(center, blast);
    // Les explosions et les gros rayons touchent aussi les véhicules.
    if blast >= 0.4 {
        let mut ev = Vec::new();
        let hit = {
            let s = &mut *server;
            s.sim.damage_bodies(center, blast, &mut ev)
        };
        if hit > 0 {
            let value: u32 = (blast as u32).saturating_mul(4);
            if let Some(c) = server.clients.get_mut(&id) {
                c.inv.reward(value);
                c.inv_dirty = true;
            }
            server.events.append(&mut ev);
            server.unsupported_for = 3.0;
            flush_events(server);
        }
    }
    if !destroyed.is_empty() {
        let value: u32 = destroyed
            .iter()
            .map(|&c| material_value(server.world.material(c)))
            .sum();
        if value > 0 {
            if let Some(c) = server.clients.get_mut(&id) {
                c.inv.reward(value);
                c.inv_dirty = true;
            }
        }
        let push = if kind == HitKind::Explosion {
            Vec3::Y * (2.0 + blast)
        } else {
            dir.normalize_or_zero() * 3.0
        };
        let mut ev = Vec::new();
        {
            let s = &mut *server;
            let ids = s.sim.add_rubble(&s.world, &destroyed, center, push);
            if !ids.is_empty() {
                ev.push(SimEvent::BlocksAdded(ids));
            }
            s.sim.wake_near(center, blast, if kind == HitKind::Explosion { 5.0 + blast * 2.5 } else { 0.0 });
        }
        let comps = if rules.gravity_collapse {
            server.world.collapse_components(&destroyed)
        } else {
            Vec::new()
        };
        for comp in &comps {
            destroyed.extend_from_slice(comp);
        }
        broadcast_removed(server, &destroyed);
        if !comps.is_empty() {
            let s = &mut *server;
            s.sim.form_bodies(&s.world, comps, &mut ev);
        }
        server.events.append(&mut ev);
        server.unsupported_for = 3.0;
        flush_events(server);
    }
    if kind == HitKind::Explosion {
        broadcast(server, Some(id), &Packet::Explode { at: center, radius: blast });
        explosion_damage(server, id, center, blast);
        let fire = exploded.map_or(0.0, |w| w.spec().incendiary);
        ignite_around(server, center, blast.max(fire));
    }
}

fn handle_command(server: &mut ServerState, id: u16, text: &str) {
    // Seul l'hôte (lobby, solo) commande ; sur un serveur dédié, la console.
    if server.host != Some(id) {
        return;
    }
    let mut parts = text.trim().splitn(2, ' ');
    let cmd = parts.next().unwrap_or("").to_lowercase();
    let arg = parts.next().unwrap_or("").trim().to_string();
    let who = name_of(server, id);
    match cmd.as_str() {
        "save" => {
            let name = (!arg.is_empty()).then_some(arg.as_str());
            match save_world(server, name) {
                Ok(path) => {
                    let msg = format!("Monde sauvegardé : {}", path.display());
                    notice(server, id, &msg);
                }
                Err(e) => notice(server, id, &format!("Sauvegarde impossible : {e}")),
            }
        }
        "reset" => {
            logger::info(format!("{who} régénère la ville"));
            let seed = crate::game::parse_seed(&arg).unwrap_or_else(random_seed);
            reset_world(server, seed);
        }
        "pause" if server.config.solo => {
            server.paused = arg != "off";
            server.last_sim = Instant::now();
        }
        _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config(dir: PathBuf, load: Option<PathBuf>) -> HostConfig {
        HostConfig {
            rules: Rules::default(),
            seed: 777,
            mode: 1,
            name: "test".into(),
            token: 1,
            dedicated: false,
            solo: true,
            world_name: "essai".into(),
            map_blocks: 9,
            motd: String::new(),
            save_dir: dir,
            load,
        }
    }

    #[test]
    fn world_survives_save_and_load() {
        let dir = std::env::temp_dir().join(format!("boomground-test-{}", std::process::id()));
        let listeners = bind(0, true).unwrap();
        let mut app = server_app(listeners, 0, config(dir.clone(), None), Arc::new(AtomicBool::new(false)), None).unwrap();
        let path;
        let cell = IVec3::new(1000, crate::map::GROUND, 1000);
        let was_solid;
        {
            let mut state = app.world_mut().resource_mut::<ServerState>();
            was_solid = state.world.is_solid(cell);
            // Une modification, une porte ouverte, un bloc de gravats.
            let center = state.world.cell_center(cell);
            let destroyed = state.world.carve(center, 1.0);
            let cells: Vec<[u16; 4]> = destroyed.iter().map(|c| [c.x as u16, c.y as u16, c.z as u16, 255]).collect();
            state.history.extend(cells);
            state.world.set(IVec3::new(5, 20, 5), true, 8);
            state.history.push([5, 20, 5, 8]);
            if !state.world.doors.is_empty() {
                state.world.toggle_door(0);
            }
            let s = &mut *state;
            s.sim.add_rubble(&s.world, &destroyed, Vec3::ZERO, Vec3::ZERO);
            path = save_world(&mut state, Some("essai")).unwrap();
        }
        assert!(path.exists());
        let before = {
            let state = app.world().resource::<ServerState>();
            (state.history.len(), state.sim.blocks.len(), state.world.alive, state.world.doors.iter().filter(|d| d.is_open).count())
        };

        let listeners = bind(0, true).unwrap();
        let app2 = server_app(listeners, 0, config(dir.clone(), Some(path)), Arc::new(AtomicBool::new(false)), None).unwrap();
        let state = app2.world().resource::<ServerState>();
        assert_eq!(state.history.len(), before.0);
        assert_eq!(state.sim.blocks.len(), before.1);
        assert_eq!(state.world.alive, before.2);
        assert_eq!(state.world.doors.iter().filter(|d| d.is_open).count(), before.3);
        assert!(state.world.is_solid(IVec3::new(5, 20, 5)));
        if was_solid {
            assert!(!state.world.is_solid(cell));
        }
        assert_eq!(state.world.seed, 777);
        let _ = std::fs::remove_dir_all(dir);
    }
}
