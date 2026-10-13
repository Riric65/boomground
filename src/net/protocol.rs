//! Protocole réseau : trames binaires préfixées par leur taille (u16).
//!
//! Le client **prédit** ses actions (réponse immédiate) puis envoie ; le
//! serveur valide et renvoie la liste **réelle** des voxels détruits. Comme
//! `carve` est idempotent, un voxel déjà cassé côté client est sans effet.
//!
//! Une partie commence par un **lobby** : le serveur envoie la liste des
//! joueurs (`Roster`) à chaque arrivée ou départ, et l'hôte lance la partie
//! (`Start`). Un joueur qui arrive après le lancement entre directement.

use std::io::{self, Read};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};

use bevy::prelude::Vec3;

/// Port TCP par défaut du serveur.
pub const DEFAULT_PORT: u16 = 28431;
/// Port UDP de découverte des parties sur le réseau local.
pub const DISCOVERY_PORT: u16 = 28432;
/// Version du protocole : deux versions différentes refusent de jouer
/// ensemble (message clair plutôt que des paquets incompréhensibles).
pub const PROTOCOL_VERSION: u16 = 5;
/// Nombre maximal de joueurs dans une partie.
pub const MAX_PLAYERS: usize = 16;

/// Ce qui circule entre le client et le serveur.
#[derive(Debug, Clone, PartialEq)]
pub enum Packet {
    /// Client → serveur : demande d'entrée. `token` identifie l'hôte (0 pour
    /// les autres joueurs).
    Join { name: String, token: u64, version: u16 },
    /// Serveur → client : identifiant, seed, mode, partie déjà lancée.
    Welcome {
        id: u16,
        seed: u64,
        mode: u8,
        started: bool,
        /// Côté de la carte, en pâtés de maisons de 64 voxels.
        map_blocks: u16,
        /// Nom du serveur et capacité.
        name: String,
        max_players: u8,
        /// Bit 0 vol autorisé, bit 1 chat, bit 2 voix de proximité.
        flags: u8,
    },
    /// Serveur → client : entrée refusée (version, partie pleine…).
    Refused { reason: String },
    /// Serveur → clients : joueurs présents et hôte.
    Roster { host: u16, players: Vec<(u16, String)> },
    /// Hôte → serveur → clients : la partie commence.
    Start,
    /// Client → serveur : position/regard (envoyé en continu).
    State {
        pos: Vec3,
        yaw: f32,
        pitch: f32,
        weapon: u8,
    },
    /// Client → serveur : action d'arme (le serveur la vérifie).
    Hit {
        kind: HitKind,
        origin: Vec3,
        dir: Vec3,
        blast: f32,
        /// Numéro du tir (les plombs d'un même tir le partagent).
        shot: u16,
    },
    /// Serveur → client : voxels réellement détruits (autorité).
    /// `rubble` : faut-il créer des gravats (pas pour l'historique envoyé à
    /// un joueur qui arrive).
    Voxels { cells: Vec<[u16; 3]>, rubble: bool },
    /// Serveur → client : autre joueur.
    Player {
        id: u16,
        pos: Vec3,
        yaw: f32,
        pitch: f32,
        weapon: u8,
    },
    /// Serveur → client : joueur parti.
    PlayerLeft { id: u16 },
    /// Bombe posée (client → serveur → autres clients).
    BombPlaced { at: Vec3, weapon: u8 },
    /// Serveur → autres clients : une explosion a eu lieu (effets).
    Explode { at: Vec3, radius: f32 },
    /// Client → serveur : pose d'un bloc (outil de construction).
    Place { cell: [u16; 3], material: u8 },
    /// Serveur → clients : blocs posés ou retirés `[x, y, z, matériau]`
    /// (matériau 255 = voxel retiré). Sert aussi aux portes.
    Placed { cells: Vec<[u16; 4]> },
    /// Client → serveur : ouvrir / fermer une porte.
    Interact { door: u16 },
    /// Serveur → clients : état d'une porte (les voxels arrivent par
    /// `Placed`).
    DoorState { door: u16, open: bool },
    /// Serveur → client : points de vie restants (0..=100) et d'où vient
    /// le coup.
    Hurt { hp: u8, from: Vec3 },
    /// Serveur → clients : un joueur est mort.
    Died { id: u16, at: Vec3, push: Vec3 },
    /// Serveur → clients : un joueur réapparaît.
    Respawned { id: u16, pos: Vec3 },
    /// Serveur → client : déplacement forcé (correction anti-triche,
    /// chargement d'une sauvegarde) sans toucher aux points de vie.
    Teleport { pos: Vec3 },
    /// Serveur → clients : nouveaux blocs de gravats.
    Blocks { list: Vec<BlockInfo> },
    /// Serveur → clients : blocs en mouvement `(id, position, lacet)`.
    BlockPose { list: Vec<(u32, Vec3, f32)> },
    /// Serveur → clients : blocs disparus.
    BlockRemove { ids: Vec<u32> },
    /// Serveur → clients : corps rigide (grille compressée, en morceaux).
    BodyDef {
        id: u32,
        dims: [u16; 3],
        com: Vec3,
        part: u16,
        total: u16,
        data: Vec<u8>,
    },
    /// Serveur → clients : positions des corps `(id, centre de masse, rotation)`.
    BodyPose { list: Vec<(u32, Vec3, [f32; 4])> },
    /// Serveur → clients : un corps s'est figé ou a disparu.
    BodyRemove { id: u32 },
    /// Serveur → clients : roues des véhicules `(id, centres des roues dans
    /// le repère du corps, braquage, rotation)`.
    Wheels { list: Vec<(u32, [Vec3; 4], f32, f32)> },
    /// Serveur → client : inventaire autoritaire.
    Inv { money: u32, earned: u32, slots: Vec<[u32; 3]> },
    /// Client → serveur : recharge l'arme en main.
    Reload,
    /// Client → serveur : achat d'une arme (`ammo` = paquet de munitions).
    Buy { weapon: u8, ammo: bool },
    /// Client → serveur : bombe lancée ou roquette tirée.
    Throw { weapon: u8 },
    /// Serveur → client : message à afficher.
    Notice { text: String },
    /// Serveur → client : expulsé (puis la connexion est fermée).
    Kicked { reason: String },
    /// Serveur → clients : impact violent (son, poussière).
    Impact { at: Vec3, power: f32 },
    /// Client (hôte) → serveur : commande (`save nom`, `reset 1234`,
    /// `pause on`…).
    Command { text: String },
    /// Serveur → clients : nouvelle carte (même seed ou nouvelle).
    Reset { seed: u64 },
    /// Client → serveur : état du jeu en pause (solo).
    Ping,
    /// Serveur → clients : voxels qui prennent feu / s'éteignent.
    Fire { add: Vec<[u16; 3]>, remove: Vec<[u16; 3]> },
    /// Client → serveur : message de chat.
    Chat { text: String },
    /// Serveur → clients : message de chat (`from` 0 = serveur).
    ChatMsg { from: u16, name: String, text: String },
    /// Voix de proximité (µ-law 8 bits, 16 kHz, 20 ms par trame). Client →
    /// serveur (`from` ignoré), puis serveur → auditeurs proches.
    Voice { from: u16, seq: u16, data: Vec<u8> },
    /// Client → serveur (sans `Join`) : demande l'état du serveur.
    Status,
    /// Serveur → client : réponse à `Status`.
    StatusInfo {
        name: String,
        players: u8,
        max: u8,
        mode: u8,
        version: u16,
        motd: String,
    },
    /// Client → serveur : commandes du véhicule conduit.
    Drive { throttle: i8, steer: i8, brake: bool },
    /// Client → serveur : monter dans le véhicule `car`.
    Enter { car: u16 },
    /// Client → serveur : quitter le véhicule.
    Exit,
    /// Serveur → client : corps conduit (0 = aucun).
    Driving { body: u32 },
}

/// Un bloc de gravats annoncé aux clients.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockInfo {
    pub id: u32,
    pub pos: Vec3,
    pub half: Vec3,
    pub yaw: f32,
    pub mat: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitKind {
    Sledge,
    Bullet,
    Explosion,
}

impl HitKind {
    fn tag(self) -> u8 {
        match self {
            HitKind::Sledge => 0,
            HitKind::Bullet => 1,
            HitKind::Explosion => 2,
        }
    }

    fn from_tag(t: u8) -> Option<Self> {
        match t {
            0 => Some(HitKind::Sledge),
            1 => Some(HitKind::Bullet),
            2 => Some(HitKind::Explosion),
            _ => None,
        }
    }
}

/// Écriture d'un corps de paquet.
struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
    fn string(&mut self, s: &str) {
        let bytes = &s.as_bytes()[..s.len().min(255)];
        // Ne jamais couper au milieu d'un caractère UTF-8.
        let mut end = bytes.len();
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        self.u16(end as u16);
        self.0.extend_from_slice(&s.as_bytes()[..end]);
    }
}

impl Packet {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer(Vec::with_capacity(64));
        match self {
            Packet::Join {
                name,
                token,
                version,
            } => {
                w.u8(0);
                w.string(name);
                w.u64(*token);
                w.u16(*version);
            }
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
                w.u8(1);
                w.u16(*id);
                w.u64(*seed);
                w.u8(*mode);
                w.u8(*started as u8);
                w.u16(*map_blocks);
                w.string(name);
                w.u8(*max_players);
                w.u8(*flags);
            }
            Packet::State {
                pos,
                yaw,
                pitch,
                weapon,
            } => {
                w.u8(2);
                w.vec3(*pos);
                w.f32(*yaw);
                w.f32(*pitch);
                w.u8(*weapon);
            }
            Packet::Hit {
                kind,
                origin,
                dir,
                blast,
                shot,
            } => {
                w.u8(3);
                w.u8(kind.tag());
                w.vec3(*origin);
                w.vec3(*dir);
                w.f32(*blast);
                w.u16(*shot);
            }
            Packet::Voxels { cells, rubble } => {
                w.u8(4);
                w.u8(*rubble as u8);
                w.u16(cells.len() as u16);
                for c in cells {
                    w.u16(c[0]);
                    w.u16(c[1]);
                    w.u16(c[2]);
                }
            }
            Packet::Player {
                id,
                pos,
                yaw,
                pitch,
                weapon,
            } => {
                w.u8(5);
                w.u16(*id);
                w.vec3(*pos);
                w.f32(*yaw);
                w.f32(*pitch);
                w.u8(*weapon);
            }
            Packet::PlayerLeft { id } => {
                w.u8(6);
                w.u16(*id);
            }
            Packet::BombPlaced { at, weapon } => {
                w.u8(7);
                w.vec3(*at);
                w.u8(*weapon);
            }
            Packet::Explode { at, radius } => {
                w.u8(8);
                w.vec3(*at);
                w.f32(*radius);
            }
            Packet::Refused { reason } => {
                w.u8(9);
                w.string(reason);
            }
            Packet::Roster { host, players } => {
                w.u8(10);
                w.u16(*host);
                w.u8(players.len().min(255) as u8);
                for (id, name) in players.iter().take(255) {
                    w.u16(*id);
                    w.string(name);
                }
            }
            Packet::Start => w.u8(11),
            Packet::Place { cell, material } => {
                w.u8(12);
                for c in cell {
                    w.u16(*c);
                }
                w.u8(*material);
            }
            Packet::Placed { cells } => {
                w.u8(13);
                w.u16(cells.len() as u16);
                for c in cells {
                    for v in c {
                        w.u16(*v);
                    }
                }
            }
            Packet::Interact { door } => {
                w.u8(14);
                w.u16(*door);
            }
            Packet::DoorState { door, open } => {
                w.u8(15);
                w.u16(*door);
                w.u8(*open as u8);
            }
            Packet::Hurt { hp, from } => {
                w.u8(16);
                w.u8(*hp);
                w.vec3(*from);
            }
            Packet::Died { id, at, push } => {
                w.u8(17);
                w.u16(*id);
                w.vec3(*at);
                w.vec3(*push);
            }
            Packet::Respawned { id, pos } => {
                w.u8(18);
                w.u16(*id);
                w.vec3(*pos);
            }
            Packet::Teleport { pos } => {
                w.u8(19);
                w.vec3(*pos);
            }
            Packet::Blocks { list } => {
                w.u8(20);
                w.u16(list.len() as u16);
                for b in list {
                    w.u32(b.id);
                    w.vec3(b.pos);
                    w.vec3(b.half);
                    w.f32(b.yaw);
                    w.u8(b.mat);
                }
            }
            Packet::BlockPose { list } => {
                w.u8(21);
                w.u16(list.len() as u16);
                for (id, pos, yaw) in list {
                    w.u32(*id);
                    w.vec3(*pos);
                    w.f32(*yaw);
                }
            }
            Packet::BlockRemove { ids } => {
                w.u8(22);
                w.u16(ids.len() as u16);
                for id in ids {
                    w.u32(*id);
                }
            }
            Packet::BodyDef {
                id,
                dims,
                com,
                part,
                total,
                data,
            } => {
                w.u8(23);
                w.u32(*id);
                for d in dims {
                    w.u16(*d);
                }
                w.vec3(*com);
                w.u16(*part);
                w.u16(*total);
                w.u16(data.len() as u16);
                w.0.extend_from_slice(data);
            }
            Packet::BodyPose { list } => {
                w.u8(24);
                w.u16(list.len() as u16);
                for (id, pos, rot) in list {
                    w.u32(*id);
                    w.vec3(*pos);
                    for q in rot {
                        w.f32(*q);
                    }
                }
            }
            Packet::Wheels { list } => {
                w.u8(46);
                w.u16(list.len() as u16);
                for (id, centers, steer, spin) in list {
                    w.u32(*id);
                    for c in centers {
                        w.vec3(*c);
                    }
                    w.f32(*steer);
                    w.f32(*spin);
                }
            }
            Packet::BodyRemove { id } => {
                w.u8(25);
                w.u32(*id);
            }
            Packet::Inv { money, earned, slots } => {
                w.u8(26);
                w.u32(*money);
                w.u32(*earned);
                w.u8(slots.len().min(255) as u8);
                for s in slots.iter().take(255) {
                    w.u32(s[0]);
                    w.u32(s[1]);
                    w.u32(s[2]);
                }
            }
            Packet::Reload => w.u8(27),
            Packet::Buy { weapon, ammo } => {
                w.u8(28);
                w.u8(*weapon);
                w.u8(*ammo as u8);
            }
            Packet::Throw { weapon } => {
                w.u8(29);
                w.u8(*weapon);
            }
            Packet::Notice { text } => {
                w.u8(30);
                w.string(text);
            }
            Packet::Kicked { reason } => {
                w.u8(31);
                w.string(reason);
            }
            Packet::Impact { at, power } => {
                w.u8(32);
                w.vec3(*at);
                w.f32(*power);
            }
            Packet::Command { text } => {
                w.u8(33);
                w.string(text);
            }
            Packet::Reset { seed } => {
                w.u8(34);
                w.u64(*seed);
            }
            Packet::Ping => w.u8(35),
            Packet::Chat { text } => {
                w.u8(37);
                w.string(text);
            }
            Packet::ChatMsg { from, name, text } => {
                w.u8(38);
                w.u16(*from);
                w.string(name);
                w.string(text);
            }
            Packet::Voice { from, seq, data } => {
                w.u8(39);
                w.u16(*from);
                w.u16(*seq);
                w.u16(data.len() as u16);
                w.0.extend_from_slice(data);
            }
            Packet::Status => w.u8(40),
            Packet::StatusInfo {
                name,
                players,
                max,
                mode,
                version,
                motd,
            } => {
                w.u8(41);
                w.string(name);
                w.u8(*players);
                w.u8(*max);
                w.u8(*mode);
                w.u16(*version);
                w.string(motd);
            }
            Packet::Drive { throttle, steer, brake } => {
                w.u8(42);
                w.u8(*throttle as u8);
                w.u8(*steer as u8);
                w.u8(*brake as u8);
            }
            Packet::Enter { car } => {
                w.u8(43);
                w.u16(*car);
            }
            Packet::Exit => w.u8(44),
            Packet::Driving { body } => {
                w.u8(45);
                w.u32(*body);
            }
            Packet::Fire { add, remove } => {
                w.u8(36);
                for list in [add, remove] {
                    w.u16(list.len() as u16);
                    for c in list {
                        w.u16(c[0]);
                        w.u16(c[1]);
                        w.u16(c[2]);
                    }
                }
            }
        }

        let body = w.0;
        let mut out = Vec::with_capacity(body.len() + 2);
        out.extend_from_slice(&(body.len() as u16).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut cur = Cursor { data: bytes, at: 0 };
        let tag = cur.u8()?;
        Some(match tag {
            0 => Packet::Join {
                name: cur.string()?,
                token: cur.u64()?,
                version: cur.u16()?,
            },
            1 => Packet::Welcome {
                id: cur.u16()?,
                seed: cur.u64()?,
                mode: cur.u8()?,
                started: cur.u8()? != 0,
                map_blocks: cur.u16()?,
                name: cur.string()?,
                max_players: cur.u8()?,
                flags: cur.u8()?,
            },
            2 => Packet::State {
                pos: cur.vec3()?,
                yaw: cur.f32()?,
                pitch: cur.f32()?,
                weapon: cur.u8()?,
            },
            3 => Packet::Hit {
                kind: HitKind::from_tag(cur.u8()?)?,
                origin: cur.vec3()?,
                dir: cur.vec3()?,
                blast: cur.f32()?,
                shot: cur.u16()?,
            },
            4 => {
                let rubble = cur.u8()? != 0;
                let n = cur.u16()?;
                let mut cells = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    cells.push([cur.u16()?, cur.u16()?, cur.u16()?]);
                }
                Packet::Voxels { cells, rubble }
            }
            5 => Packet::Player {
                id: cur.u16()?,
                pos: cur.vec3()?,
                yaw: cur.f32()?,
                pitch: cur.f32()?,
                weapon: cur.u8()?,
            },
            6 => Packet::PlayerLeft { id: cur.u16()? },
            7 => Packet::BombPlaced {
                at: cur.vec3()?,
                weapon: cur.u8()?,
            },
            8 => Packet::Explode {
                at: cur.vec3()?,
                radius: cur.f32()?,
            },
            9 => Packet::Refused {
                reason: cur.string()?,
            },
            10 => {
                let host = cur.u16()?;
                let n = cur.u8()?;
                let mut players = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    players.push((cur.u16()?, cur.string()?));
                }
                Packet::Roster { host, players }
            }
            11 => Packet::Start,
            12 => Packet::Place {
                cell: [cur.u16()?, cur.u16()?, cur.u16()?],
                material: cur.u8()?,
            },
            13 => {
                let n = cur.u16()?;
                let mut cells = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    cells.push([cur.u16()?, cur.u16()?, cur.u16()?, cur.u16()?]);
                }
                Packet::Placed { cells }
            }
            14 => Packet::Interact { door: cur.u16()? },
            15 => Packet::DoorState {
                door: cur.u16()?,
                open: cur.u8()? != 0,
            },
            16 => Packet::Hurt {
                hp: cur.u8()?,
                from: cur.vec3()?,
            },
            17 => Packet::Died {
                id: cur.u16()?,
                at: cur.vec3()?,
                push: cur.vec3()?,
            },
            18 => Packet::Respawned {
                id: cur.u16()?,
                pos: cur.vec3()?,
            },
            19 => Packet::Teleport { pos: cur.vec3()? },
            20 => {
                let n = cur.u16()?;
                let mut list = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    list.push(BlockInfo {
                        id: cur.u32()?,
                        pos: cur.vec3()?,
                        half: cur.vec3()?,
                        yaw: cur.f32()?,
                        mat: cur.u8()?,
                    });
                }
                Packet::Blocks { list }
            }
            21 => {
                let n = cur.u16()?;
                let mut list = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    list.push((cur.u32()?, cur.vec3()?, cur.f32()?));
                }
                Packet::BlockPose { list }
            }
            22 => {
                let n = cur.u16()?;
                let mut ids = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    ids.push(cur.u32()?);
                }
                Packet::BlockRemove { ids }
            }
            23 => {
                let id = cur.u32()?;
                let dims = [cur.u16()?, cur.u16()?, cur.u16()?];
                let com = cur.vec3()?;
                let part = cur.u16()?;
                let total = cur.u16()?;
                let n = cur.u16()? as usize;
                Packet::BodyDef {
                    id,
                    dims,
                    com,
                    part,
                    total,
                    data: cur.take(n)?.to_vec(),
                }
            }
            24 => {
                let n = cur.u16()?;
                let mut list = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    let id = cur.u32()?;
                    let pos = cur.vec3()?;
                    list.push((id, pos, [cur.f32()?, cur.f32()?, cur.f32()?, cur.f32()?]));
                }
                Packet::BodyPose { list }
            }
            25 => Packet::BodyRemove { id: cur.u32()? },
            46 => {
                let n = cur.u16()?;
                let mut list = Vec::with_capacity(n.min(512) as usize);
                for _ in 0..n {
                    let id = cur.u32()?;
                    let centers = [cur.vec3()?, cur.vec3()?, cur.vec3()?, cur.vec3()?];
                    let steer = cur.f32()?;
                    let spin = cur.f32()?;
                    if !steer.is_finite() || !spin.is_finite() || centers.iter().any(|c| !c.is_finite()) {
                        return None;
                    }
                    list.push((id, centers, steer, spin));
                }
                Packet::Wheels { list }
            }
            26 => {
                let money = cur.u32()?;
                let earned = cur.u32()?;
                let n = cur.u8()?;
                let mut slots = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    slots.push([cur.u32()?, cur.u32()?, cur.u32()?]);
                }
                Packet::Inv { money, earned, slots }
            }
            27 => Packet::Reload,
            28 => Packet::Buy {
                weapon: cur.u8()?,
                ammo: cur.u8()? != 0,
            },
            29 => Packet::Throw { weapon: cur.u8()? },
            30 => Packet::Notice { text: cur.string()? },
            31 => Packet::Kicked { reason: cur.string()? },
            32 => Packet::Impact {
                at: cur.vec3()?,
                power: cur.f32()?,
            },
            33 => Packet::Command { text: cur.string()? },
            34 => Packet::Reset { seed: cur.u64()? },
            35 => Packet::Ping,
            37 => Packet::Chat { text: cur.string()? },
            38 => Packet::ChatMsg {
                from: cur.u16()?,
                name: cur.string()?,
                text: cur.string()?,
            },
            39 => {
                let from = cur.u16()?;
                let seq = cur.u16()?;
                let n = cur.u16()? as usize;
                Packet::Voice {
                    from,
                    seq,
                    data: cur.take(n)?.to_vec(),
                }
            }
            40 => Packet::Status,
            41 => Packet::StatusInfo {
                name: cur.string()?,
                players: cur.u8()?,
                max: cur.u8()?,
                mode: cur.u8()?,
                version: cur.u16()?,
                motd: cur.string()?,
            },
            42 => Packet::Drive {
                throttle: cur.u8()? as i8,
                steer: cur.u8()? as i8,
                brake: cur.u8()? != 0,
            },
            43 => Packet::Enter { car: cur.u16()? },
            44 => Packet::Exit,
            45 => Packet::Driving { body: cur.u32()? },
            36 => {
                let mut lists: [Vec<[u16; 3]>; 2] = [Vec::new(), Vec::new()];
                for list in lists.iter_mut() {
                    let n = cur.u16()?;
                    for _ in 0..n {
                        list.push([cur.u16()?, cur.u16()?, cur.u16()?]);
                    }
                }
                let [add, remove] = lists;
                Packet::Fire { add, remove }
            }
            _ => return None,
        })
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let slice = self.data.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn vec3(&mut self) -> Option<Vec3> {
        Some(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    fn string(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }
}

/// Analyse un flux TCP : rend les trames complètes vues jusqu'ici.
#[derive(Default)]
pub struct FrameReader {
    buffer: Vec<u8>,
}

impl FrameReader {
    pub fn push(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Vide le tampon et renvoie toutes les trames complètes.
    pub fn drain(&mut self) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        while let Some(frame) = self.next() {
            frames.push(frame);
        }
        frames
    }

    /// Retire et renvoie la prochaine trame complète.
    pub fn next(&mut self) -> Option<Vec<u8>> {
        if self.buffer.len() < 2 {
            return None;
        }
        let len = u16::from_le_bytes([self.buffer[0], self.buffer[1]]) as usize;
        if self.buffer.len() < 2 + len {
            return None;
        }
        let frame = self.buffer[2..2 + len].to_vec();
        self.buffer.drain(..2 + len);
        Some(frame)
    }
}

pub fn read_into(stream: &mut TcpStream, into: &mut FrameReader) -> io::Result<usize> {
    let mut chunk = [0u8; 16 * 1024];
    let n = stream.read(&mut chunk)?;
    if n > 0 {
        into.push(&chunk[..n]);
    }
    Ok(n)
}

/// Découpe `adresse` en hôte + port (port par défaut si absent). Gère
/// `[ipv6]:port`.
pub fn split_address(address: &str) -> (String, u16) {
    let address = address.trim();
    if let Some(rest) = address.strip_prefix('[') {
        if let Some((host, tail)) = rest.split_once(']') {
            let port = tail
                .strip_prefix(':')
                .and_then(|p| p.parse().ok())
                .unwrap_or(DEFAULT_PORT);
            return (host.to_string(), port);
        }
    }
    // Une IPv6 nue contient plusieurs « : » : pas de port.
    if address.matches(':').count() > 1 {
        return (address.to_string(), DEFAULT_PORT);
    }
    if let Some((host, port)) = address.rsplit_once(':') {
        if let Ok(port) = port.parse::<u16>() {
            return (host.to_string(), port);
        }
    }
    (address.to_string(), DEFAULT_PORT)
}

/// Nombre maximal de cellules par paquet `Voxels` (la trame fait au plus
/// 65 535 octets).
pub const MAX_CELLS_PER_PACKET: usize = 10_000;
/// Idem pour `Placed` (8 octets par cellule).
pub const MAX_PLACED_PER_PACKET: usize = 7_000;

// ---------------------------------------------------------------------------
// Codes de partie
// ---------------------------------------------------------------------------

/// Alphabet sans lettres ambiguës (pas de I, L, O, U).
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Code de partie à partager : l'adresse IPv4 et le port, écrits en
/// 10 caractères lisibles (`K7M2Q-9XH4C`).
pub fn encode_code(addr: SocketAddrV4) -> String {
    let ip = u32::from(*addr.ip()) as u64;
    let value = (ip << 16) | addr.port() as u64;
    let mut chars = [0u8; 10];
    for (i, c) in chars.iter_mut().enumerate() {
        let shift = 5 * (9 - i);
        *c = ALPHABET[((value >> shift) & 31) as usize];
    }
    let s = String::from_utf8(chars.to_vec()).unwrap_or_default();
    format!("{}-{}", &s[..5], &s[5..])
}

/// Relit un code de partie (insensible à la casse, tirets et espaces
/// ignorés, O/I/L tolérés). `None` si ce n'est pas un code.
pub fn decode_code(text: &str) -> Option<SocketAddrV4> {
    let cleaned: Vec<u8> = text
        .chars()
        .filter(|c| !matches!(c, '-' | ' '))
        .map(|c| match c.to_ascii_uppercase() {
            'O' => b'0',
            'I' | 'L' => b'1',
            other => other as u8,
        })
        .collect();
    if cleaned.len() != 10 {
        return None;
    }
    let mut value: u64 = 0;
    for b in cleaned {
        let digit = ALPHABET.iter().position(|&a| a == b)? as u64;
        value = (value << 5) | digit;
    }
    if value >> 48 != 0 {
        return None;
    }
    let ip = Ipv4Addr::from((value >> 16) as u32);
    let port = (value & 0xFFFF) as u16;
    if port == 0 || ip.is_unspecified() {
        return None;
    }
    Some(SocketAddrV4::new(ip, port))
}

/// Ce que l'utilisateur a saisi pour rejoindre : code de partie ou adresse.
pub fn resolve_target(text: &str) -> String {
    match decode_code(text) {
        Some(addr) => SocketAddr::V4(addr).to_string(),
        None => text.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packets_round_trip() {
        let packets = [
            Packet::Join {
                name: "Héloïse".into(),
                token: 42,
                version: PROTOCOL_VERSION,
            },
            Packet::Welcome {
                id: 3,
                seed: 123_456_789,
                mode: 1,
                started: true,
                map_blocks: 17,
                name: "Serveur".into(),
                max_players: 16,
                flags: 5,
            },
            Packet::Chat { text: "salut".into() },
            Packet::ChatMsg { from: 2, name: "Sam".into(), text: "yo".into() },
            Packet::Voice { from: 1, seq: 9, data: vec![1, 2, 3] },
            Packet::Status,
            Packet::StatusInfo { name: "S".into(), players: 2, max: 16, mode: 0, version: 5, motd: "hi".into() },
            Packet::Drive { throttle: -1, steer: 1, brake: true },
            Packet::Enter { car: 4 },
            Packet::Exit,
            Packet::Driving { body: 77 },
            Packet::Refused { reason: "pleine".into() },
            Packet::Roster {
                host: 1,
                players: vec![(1, "Alex".into()), (2, "Sam".into())],
            },
            Packet::Start,
            Packet::Voxels {
                cells: vec![[1, 2, 3], [300, 40, 383]],
                rubble: true,
            },
            Packet::BombPlaced {
                at: Vec3::new(1.0, 2.0, 3.0),
                weapon: 11,
            },
            Packet::Explode {
                at: Vec3::ONE,
                radius: 7.0,
            },
        ];
        for p in packets {
            let bytes = p.encode();
            let mut reader = FrameReader::default();
            reader.push(&bytes);
            let frame = reader.next().expect("trame complète");
            assert_eq!(Packet::decode(&frame).expect("décodage"), p);
        }
    }

    #[test]
    fn biggest_voxel_packet_fits_in_a_frame() {
        let cells = vec![[0u16; 3]; MAX_CELLS_PER_PACKET];
        let bytes = Packet::Voxels { cells, rubble: false }.encode();
        assert!(bytes.len() - 2 <= u16::MAX as usize);
    }

    #[test]
    fn game_codes_round_trip() {
        for addr in [
            SocketAddrV4::new(Ipv4Addr::new(82, 64, 120, 7), DEFAULT_PORT),
            SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 23), 3000),
            SocketAddrV4::new(Ipv4Addr::new(255, 255, 255, 254), 65535),
        ] {
            let code = encode_code(addr);
            assert_eq!(code.len(), 11);
            assert_eq!(decode_code(&code), Some(addr));
            assert_eq!(decode_code(&code.to_lowercase().replace('-', " ")), Some(addr));
        }
        assert_eq!(decode_code("192.168.1.2"), None);
        assert_eq!(decode_code("localhost"), None);
        assert_eq!(resolve_target("monserveur.fr"), "monserveur.fr");
    }

    #[test]
    fn addresses_split() {
        assert_eq!(split_address("a.fr"), ("a.fr".into(), DEFAULT_PORT));
        assert_eq!(split_address("1.2.3.4:99"), ("1.2.3.4".into(), 99));
        assert_eq!(split_address("[::1]:5"), ("::1".into(), 5));
        assert_eq!(split_address("fe80::1"), ("fe80::1".into(), DEFAULT_PORT));
    }

    #[test]
    fn new_packets_round_trip() {
        for p in [
            Packet::Place { cell: [1, 2, 3], material: 9 },
            Packet::Placed { cells: vec![[1, 2, 3, 255], [4, 5, 6, 8]] },
            Packet::Interact { door: 7 },
            Packet::DoorState { door: 7, open: true },
            Packet::Hurt { hp: 40, from: Vec3::new(1.0, 2.0, 3.0) },
            Packet::Died { id: 3, at: Vec3::X, push: Vec3::Y },
            Packet::Respawned { id: 3, pos: Vec3::Z },
        ] {
            let bytes = p.encode();
            assert_eq!(Packet::decode(&bytes[2..]), Some(p));
        }
    }
}
