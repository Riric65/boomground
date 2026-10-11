//! Protocole réseau : trames binaires préfixées par leur taille (u16).
//!
//! Le client **prédit** ses actions (réponse immédiate) puis envoie ; le
//! serveur valide et renvoie la liste **réelle** des voxels détruits. Comme
//! `carve` est idempotent, un voxel déjà cassé côté client est sans effet.

use std::io::{self, Read, Write};
use std::net::TcpStream;

use bevy::prelude::Vec3;

/// Port par défaut du serveur.
pub const DEFAULT_PORT: u16 = 28431;

/// Ce qui circule entre le client et le serveur.
#[derive(Debug, Clone)]
pub enum Packet {
    /// Client → serveur : demande d'entrée.
    Join { name: String },
    /// Serveur → client : identifiant attribué.
    Welcome { id: u16 },
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
    Voxels { cells: Vec<[u16; 3]> },
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
    /// Client → serveur : la C4 est posée.
    C4Placed { at: Vec3 },
    /// Serveur → client : la C4 a détoné.
    Explode { at: Vec3, radius: f32 },
    /// Serveur → client : gravats (apparition / disparition). La physique
    /// reste locale, mais tout le monde voit les mêmes blocs.
    Chunks {
        spawned: Vec<ChunkState>,
        removed: Vec<[i16; 3]>,
    },
}

/// Un bloc de gravats tel qu'il est décrit par le serveur.
#[derive(Debug, Clone, Copy)]
pub struct ChunkState {
    pub key: [i16; 3],
    pub at: [f32; 3],
    pub material: u8,
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

impl Packet {
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::with_capacity(64);
        match self {
            Packet::Join { name } => {
                body.push(0);
                let bytes = name.as_bytes();
                body.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
                body.extend_from_slice(bytes);
            }
            Packet::Welcome { id } => {
                body.push(1);
                body.extend_from_slice(&id.to_le_bytes());
            }
            Packet::State {
                pos,
                yaw,
                pitch,
                weapon,
            } => {
                body.push(2);
                write_vec3(&mut body, *pos);
                body.extend_from_slice(&yaw.to_le_bytes());
                body.extend_from_slice(&pitch.to_le_bytes());
                body.push(*weapon);
            }
            Packet::Hit {
                kind,
                origin,
                dir,
                blast,
            } => {
                body.push(3);
                body.push(kind.tag());
                write_vec3(&mut body, *origin);
                write_vec3(&mut body, *dir);
                body.extend_from_slice(&blast.to_le_bytes());
            }
            Packet::Voxels { cells } => {
                body.push(4);
                body.extend_from_slice(&(cells.len() as u16).to_le_bytes());
                for c in cells {
                    body.extend_from_slice(&c[0].to_le_bytes());
                    body.extend_from_slice(&c[1].to_le_bytes());
                    body.extend_from_slice(&c[2].to_le_bytes());
                }
            }
            Packet::Player {
                id,
                pos,
                yaw,
                pitch,
                weapon,
            } => {
                body.push(5);
                body.extend_from_slice(&id.to_le_bytes());
                write_vec3(&mut body, *pos);
                body.extend_from_slice(&yaw.to_le_bytes());
                body.extend_from_slice(&pitch.to_le_bytes());
                body.push(*weapon);
            }
            Packet::PlayerLeft { id } => {
                body.push(6);
                body.extend_from_slice(&id.to_le_bytes());
            }
            Packet::C4Placed { at } => {
                body.push(7);
                write_vec3(&mut body, *at);
            }
            Packet::Explode { at, radius } => {
                body.push(8);
                write_vec3(&mut body, *at);
                body.extend_from_slice(&radius.to_le_bytes());
            }
            Packet::Chunks { spawned, removed } => {
                body.push(9);
                body.extend_from_slice(&(spawned.len() as u16).to_le_bytes());
                for c in spawned {
                    body.extend_from_slice(&c.key[0].to_le_bytes());
                    body.extend_from_slice(&c.key[1].to_le_bytes());
                    body.extend_from_slice(&c.key[2].to_le_bytes());
                    body.extend_from_slice(&c.at[0].to_le_bytes());
                    body.extend_from_slice(&c.at[1].to_le_bytes());
                    body.extend_from_slice(&c.at[2].to_le_bytes());
                    body.push(c.material);
                }
                body.extend_from_slice(&(removed.len() as u16).to_le_bytes());
                for key in removed {
                    body.extend_from_slice(&key[0].to_le_bytes());
                    body.extend_from_slice(&key[1].to_le_bytes());
                    body.extend_from_slice(&key[2].to_le_bytes());
                }
            }
        }

        let mut out = Vec::with_capacity(body.len() + 2);
        out.extend_from_slice(&(body.len() as u16).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut cur = Cursor { data: bytes, at: 0 };
        let tag = cur.u8()?;
        Some(match tag {
            0 => {
                let name = cur.string()?;
                Packet::Join { name }
            }
            1 => Packet::Welcome { id: cur.u16()? },
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
                let n = cur.u16()?;
                let mut cells = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    cells.push([cur.u16()?, cur.u16()?, cur.u16()?]);
                }
                Packet::Voxels { cells }
            }
            5 => Packet::Player {
                id: cur.u16()?,
                pos: cur.vec3()?,
                yaw: cur.f32()?,
                pitch: cur.f32()?,
                weapon: cur.u8()?,
            },
            6 => Packet::PlayerLeft { id: cur.u16()? },
            7 => Packet::C4Placed { at: cur.vec3()? },
            8 => Packet::Explode {
                at: cur.vec3()?,
                radius: cur.f32()?,
            },
            9 => {
                let n = cur.u16()?;
                let mut spawned = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    spawned.push(ChunkState {
                        key: [cur.u16()? as i16, cur.u16()? as i16, cur.u16()? as i16],
                        at: [cur.f32()?, cur.f32()?, cur.f32()?],
                        material: cur.u8()?,
                    });
                }
                let n = cur.u16()?;
                let mut removed = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    removed.push([cur.u16()? as i16, cur.u16()? as i16, cur.u16()? as i16]);
                }
                Packet::Chunks { spawned, removed }
            }
            _ => return None,
        })
    }
}

fn write_vec3(out: &mut Vec<u8>, v: Vec3) {
    out.extend_from_slice(&v.x.to_le_bytes());
    out.extend_from_slice(&v.y.to_le_bytes());
    out.extend_from_slice(&v.z.to_le_bytes());
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

pub fn write_packet(stream: &mut TcpStream, packet: &Packet) -> io::Result<()> {
    stream.write_all(&packet.encode())
}

pub fn read_into(stream: &mut TcpStream, into: &mut FrameReader) -> io::Result<usize> {
    let mut chunk = [0u8; 4096];
    let n = stream.read(&mut chunk)?;
    if n > 0 {
        into.push(&chunk[..n]);
    }
    Ok(n)
}

/// Découpe `adresse` en hôte + port (port par défaut si absent).
pub fn split_address(address: &str) -> (String, u16) {
    let address = address.trim();
    if let Some((host, port)) = address.rsplit_once(':') {
        if let Ok(port) = port.parse::<u16>() {
            return (host.to_string(), port);
        }
    }
    (address.to_string(), DEFAULT_PORT)
}