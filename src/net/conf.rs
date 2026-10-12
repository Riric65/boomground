//! `server.conf` : réglages du serveur dédié, générés au premier lancement.

use std::path::{Path, PathBuf};

use crate::game::{WeaponId, ALL_WEAPONS};
use crate::net::protocol::{DEFAULT_PORT, MAX_PLAYERS};

/// Règles d'une partie.
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
    /// Dégâts de chute.
    pub fall_damage: bool,
    /// Un bâtiment dont la base est détruite s'écroule.
    pub gravity_collapse: bool,
    /// Les joueurs peuvent poser des blocs.
    pub allow_building: bool,
    /// Les portes sont interactives.
    pub allow_doors: bool,
    /// Armes interdites.
    pub disabled_weapons: Vec<WeaponId>,
    /// Argent de départ (mode normal).
    pub starting_money: u32,
    /// Garder l'inventaire après une mort.
    pub keep_inventory: bool,
    /// Protection après réapparition (secondes).
    pub spawn_protection: f32,
    /// Secondes d'inactivité avant expulsion (0 = jamais).
    pub idle_kick: f32,
    /// Sauvegarde automatique toutes les N minutes (0 = jamais).
    pub autosave_minutes: f32,
    /// Sévérité de l'anti-triche : 0 aucun, 1 normal, 2 strict.
    pub anticheat: u8,
    /// Connexions simultanées par adresse IP.
    pub max_per_ip: usize,
    /// Rayon maximal d'une explosion, en multiple de celui de l'arme.
    pub explosion_scale: f32,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            max_players: MAX_PLAYERS,
            pvp: true,
            damage_scale: 1.0,
            respawn: crate::game::RESPAWN_TIME,
            fall_damage: true,
            gravity_collapse: true,
            allow_building: true,
            allow_doors: true,
            disabled_weapons: Vec::new(),
            starting_money: 0,
            keep_inventory: true,
            spawn_protection: 2.0,
            idle_kick: 0.0,
            autosave_minutes: 5.0,
            anticheat: 1,
            max_per_ip: 4,
            explosion_scale: 1.0,
        }
    }
}

impl Rules {
    pub fn weapon_enabled(&self, w: WeaponId) -> bool {
        !self.disabled_weapons.contains(&w)
    }
}

/// Contenu du fichier généré.
const TEMPLATE: &str = "\
# BoomGround — configuration du serveur dédié
# Modifie ce fichier puis relance le serveur. Les lignes qui commencent par
# # sont des commentaires. Supprime le fichier pour le régénérer.
# Les variables BOOM_PORT, BOOM_SEED, BOOM_MODE, BOOM_NAME priment sur le fichier.

# ---- Serveur -------------------------------------------------------------
# Nom affiché dans la liste des parties (réseau local).
name = Serveur BoomGround
# Port TCP d'écoute (ouvre-le sur ta box / ton pare-feu pour jouer en ligne).
port = 28431
# Nombre maximal de joueurs (1 à 16).
max_players = 16
# Connexions simultanées autorisées par adresse IP.
max_per_ip = 4
# Expulse les joueurs inactifs après N minutes (0 = jamais).
idle_kick_minutes = 0
# Niveau du journal : debug, info, warn, error.
log_level = info

# ---- Monde ---------------------------------------------------------------
# Nom du monde : le fichier worlds/<nom>.bgw est chargé s'il existe, créé
# sinon. Il contient TOUT (ville modifiée, débris, corps en chute, joueurs).
world = world
# Graine de la ville pour un NOUVEAU monde : nombre, mot, ou random.
seed = random
# Mode de jeu : sandbox (tout débloqué) ou normal (argent et boutique).
mode = sandbox
# Sauvegarde automatique toutes les N minutes (0 = jamais). Le monde est
# aussi sauvegardé par la commande « save » et à l'arrêt (« stop »).
autosave_minutes = 5

# ---- Règles --------------------------------------------------------------
# Les joueurs peuvent se blesser entre eux (les explosions restent
# dangereuses pour leur auteur).
pvp = true
# Multiplicateur de dégâts : 1 = normal, 0 = invulnérable, 2 = double.
damage_scale = 1.0
# Dégâts de chute.
fall_damage = true
# Secondes avant la réapparition d'un joueur mort.
respawn_seconds = 5
# Secondes d'invulnérabilité après la réapparition.
spawn_protection = 2
# Garde l'inventaire (armes, munitions) après une mort.
keep_inventory = true
# Un bâtiment dont la base est détruite s'écroule (corps rigides). Si
# false, les ruines restent en l'air comme dans un jeu d'avant.
gravity_collapse = true
# Les joueurs peuvent poser des blocs (outil Blocs).
allow_building = true
# Les portes sont interactives.
allow_doors = true
# Armes interdites, séparées par des virgules. Valeurs possibles :
# hammer, sledge, warhammer, pickaxe, jackhammer, glock, revolver, smg,
# shotgun, rifle, minigun, sniper, railgun, rocket, grenade, dynamite, c4,
# megabomb, blocks
disabled_weapons =
# Argent de départ (mode normal).
starting_money = 0
# Multiplicateur du rayon maximal des explosions (1 = valeurs du jeu).
explosion_scale = 1.0

# ---- Anti-triche ---------------------------------------------------------
# 0 = aucun contrôle de déplacement, 1 = normal, 2 = strict (expulse plus vite).
anticheat = 1
";

/// Réglages lus dans `server.conf`.
pub struct ServerConf {
    pub name: String,
    pub port: u16,
    pub seed: Option<u64>,
    pub mode: u8,
    pub world: String,
    pub rules: Rules,
    pub log_level: crate::net::logger::Level,
}

/// Dossier du serveur : à côté de l'exécutable (sinon dossier courant).
pub fn base_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn load() -> ServerConf {
    let path = base_dir().join("server.conf");
    let created = !path.exists();
    if created {
        let _ = std::fs::write(&path, TEMPLATE);
    }
    let mut conf = ServerConf {
        name: "Serveur BoomGround".into(),
        port: DEFAULT_PORT,
        seed: None,
        mode: 0,
        world: "world".into(),
        rules: Rules::default(),
        log_level: crate::net::logger::Level::Info,
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return conf;
    };
    let mut problems = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            problems.push(format!("server.conf ligne {} ignorée : {line}", n + 1));
            continue;
        };
        let (key, value) = (key.trim().to_lowercase(), value.trim());
        let flag = |default: bool| match value.to_lowercase().as_str() {
            "true" | "1" | "yes" | "oui" | "on" => true,
            "false" | "0" | "no" | "non" | "off" => false,
            _ => default,
        };
        let r = &mut conf.rules;
        match key.as_str() {
            "name" if !value.is_empty() => conf.name = value.chars().take(40).collect(),
            "port" => conf.port = value.parse().unwrap_or(conf.port),
            "seed" => {
                conf.seed = if value.eq_ignore_ascii_case("random") || value.is_empty() {
                    None
                } else {
                    crate::game::parse_seed(value)
                }
            }
            "mode" => conf.mode = u8::from(value.eq_ignore_ascii_case("normal")),
            "world" => conf.world = crate::net::save::file_stem(value),
            "max_players" => r.max_players = value.parse().unwrap_or(r.max_players).clamp(1, MAX_PLAYERS),
            "max_per_ip" => r.max_per_ip = value.parse().unwrap_or(r.max_per_ip).clamp(1, 16),
            "idle_kick_minutes" => r.idle_kick = value.parse::<f32>().unwrap_or(0.0).clamp(0.0, 1440.0) * 60.0,
            "log_level" => {
                conf.log_level = match value.to_lowercase().as_str() {
                    "debug" => crate::net::logger::Level::Debug,
                    "warn" => crate::net::logger::Level::Warn,
                    "error" => crate::net::logger::Level::Error,
                    _ => crate::net::logger::Level::Info,
                }
            }
            "autosave_minutes" => r.autosave_minutes = value.parse().unwrap_or(5.0f32).clamp(0.0, 1440.0),
            "pvp" => r.pvp = flag(r.pvp),
            "damage_scale" => r.damage_scale = value.parse().unwrap_or(r.damage_scale).clamp(0.0, 10.0),
            "fall_damage" => r.fall_damage = flag(r.fall_damage),
            "respawn_seconds" => r.respawn = value.parse().unwrap_or(r.respawn).clamp(1.0, 60.0),
            "spawn_protection" => r.spawn_protection = value.parse().unwrap_or(r.spawn_protection).clamp(0.0, 30.0),
            "keep_inventory" => r.keep_inventory = flag(r.keep_inventory),
            "gravity_collapse" => r.gravity_collapse = flag(r.gravity_collapse),
            "allow_building" => r.allow_building = flag(r.allow_building),
            "allow_doors" => r.allow_doors = flag(r.allow_doors),
            "starting_money" => r.starting_money = value.parse().unwrap_or(0),
            "explosion_scale" => r.explosion_scale = value.parse::<f32>().unwrap_or(1.0).clamp(0.1, 4.0),
            "anticheat" => r.anticheat = value.parse().unwrap_or(1).min(2),
            "disabled_weapons" => {
                r.disabled_weapons.clear();
                for part in value.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                    match WeaponId::from_key(part) {
                        Some(w) => r.disabled_weapons.push(w),
                        None => problems.push(format!(
                            "server.conf : arme inconnue « {part} » (valeurs : {})",
                            ALL_WEAPONS.iter().map(|w| w.key()).collect::<Vec<_>>().join(", ")
                        )),
                    }
                }
            }
            other => problems.push(format!("server.conf : clé inconnue « {other} »")),
        }
    }
    for p in problems {
        eprintln!("{p}");
    }
    conf
}
