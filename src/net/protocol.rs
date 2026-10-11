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
pub const PROTOCOL_VERSION: u16 = 2;
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
            } => {
                w.u8(1);
                w.u16(*id);
                w.u64(*seed);
                w.u8(*mode);
                w.u8(*started as u8);
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
            } => {
                w.u8(3);
                w.u8(kind.tag());
                w.vec3(*origin);
                w.vec3(*dir);
                w.f32(*blast);
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
            },
            Packet::Refused { reason: "pleine".into() },
            Packet::Roster {
                host: 1,
                players: vec![(1, "L3Lunee".into()), (2, "Sam".into())],
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
}
