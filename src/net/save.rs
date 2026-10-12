//! Sauvegarde d'un monde : un fichier binaire (`.bgw`) qui contient **tout**.
//!
//! - la seed, le mode et la version de la carte ;
//! - l'historique complet des modifications du monde (voxels détruits,
//!   blocs posés, portes déplacées, corps figés) ;
//! - l'état des portes ;
//! - les gravats et les corps rigides en mouvement (positions, vitesses,
//!   voxels) ;
//! - les joueurs connus : position, santé, arme, argent, munitions.
//!
//! Le monde se reconstruit en regénérant la ville depuis la seed puis en
//! rejouant l'historique ; il n'y a donc aucune perte.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;

use crate::sim::{R, W};

const MAGIC: &[u8; 4] = b"BGW1";
const FORMAT: u32 = 1;

/// Données d'un joueur enregistrées avec le monde.
#[derive(Clone, Debug, Default)]
pub struct PlayerSave {
    pub pos: Vec3,
    pub hp: f32,
    pub weapon: u8,
    pub money: u32,
    pub earned: u32,
    /// `(possédée, chargeur, réserve)` par arme, dans l'ordre de l'arsenal.
    pub slots: Vec<[u32; 3]>,
}

/// Contenu d'un fichier de sauvegarde.
#[derive(Default)]
pub struct SaveData {
    pub name: String,
    pub saved_at: u64,
    pub map_version: u32,
    pub seed: u64,
    pub mode: u8,
    pub history: Vec<[u16; 4]>,
    pub doors_open: Vec<u32>,
    pub sim: Vec<u8>,
    pub players: BTreeMap<String, PlayerSave>,
}

fn checksum(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

impl SaveData {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = W::default();
        w.0.extend_from_slice(MAGIC);
        w.u32(FORMAT);
        w.u32(self.map_version);
        w.u64(self.seed);
        w.u8(self.mode);
        w.u64(self.saved_at);
        w.string(&self.name);
        w.u32(self.history.len() as u32);
        for e in &self.history {
            for v in e {
                w.u16(*v);
            }
        }
        w.u32(self.doors_open.len() as u32);
        for d in &self.doors_open {
            w.u32(*d);
        }
        w.bytes(&self.sim);
        w.u32(self.players.len() as u32);
        for (name, p) in &self.players {
            w.string(name);
            w.vec3(p.pos);
            w.f32(p.hp);
            w.u8(p.weapon);
            w.u32(p.money);
            w.u32(p.earned);
            w.u8(p.slots.len().min(255) as u8);
            for s in p.slots.iter().take(255) {
                w.u32(s[0]);
                w.u32(s[1]);
                w.u32(s[2]);
            }
        }
        let sum = checksum(&w.0);
        w.u64(sum);
        w.0
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 16 || &bytes[..4] != MAGIC {
            return Err("ce n'est pas un monde BoomGround".into());
        }
        let (body, tail) = bytes.split_at(bytes.len() - 8);
        let expect = u64::from_le_bytes(tail.try_into().map_err(|_| "fichier tronqué")?);
        if checksum(body) != expect {
            return Err("fichier corrompu (somme de contrôle invalide)".into());
        }
        let bad = || "fichier incomplet".to_string();
        let mut r = R::new(&body[4..]);
        let format = r.u32().ok_or_else(bad)?;
        if format != FORMAT {
            return Err(format!("format {format} non pris en charge"));
        }
        let mut s = SaveData {
            map_version: r.u32().ok_or_else(bad)?,
            seed: r.u64().ok_or_else(bad)?,
            mode: r.u8().ok_or_else(bad)?,
            saved_at: r.u64().ok_or_else(bad)?,
            name: r.string().ok_or_else(bad)?,
            ..Default::default()
        };
        let n = r.u32().ok_or_else(bad)? as usize;
        s.history.reserve(n.min(10_000_000));
        for _ in 0..n {
            s.history
                .push([r.u16().ok_or_else(bad)?, r.u16().ok_or_else(bad)?, r.u16().ok_or_else(bad)?, r.u16().ok_or_else(bad)?]);
        }
        let n = r.u32().ok_or_else(bad)? as usize;
        for _ in 0..n {
            s.doors_open.push(r.u32().ok_or_else(bad)?);
        }
        s.sim = r.bytes().ok_or_else(bad)?.to_vec();
        let n = r.u32().ok_or_else(bad)? as usize;
        for _ in 0..n {
            let name = r.string().ok_or_else(bad)?;
            let mut p = PlayerSave {
                pos: r.vec3().ok_or_else(bad)?,
                hp: r.f32().ok_or_else(bad)?,
                weapon: r.u8().ok_or_else(bad)?,
                money: r.u32().ok_or_else(bad)?,
                earned: r.u32().ok_or_else(bad)?,
                slots: Vec::new(),
            };
            let k = r.u8().ok_or_else(bad)?;
            for _ in 0..k {
                p.slots
                    .push([r.u32().ok_or_else(bad)?, r.u32().ok_or_else(bad)?, r.u32().ok_or_else(bad)?]);
            }
            s.players.insert(name, p);
        }
        Ok(s)
    }

    pub fn write_file(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // Écriture atomique : fichier temporaire puis renommage.
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.encode()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    pub fn read_file(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("lecture impossible : {e}"))?;
        Self::decode(&bytes)
    }
}

/// Nom de fichier sûr à partir d'un nom saisi.
pub fn file_stem(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(40)
        .collect();
    if s.is_empty() {
        "monde".into()
    } else {
        s
    }
}

/// Mondes enregistrés dans `dir` : `(nom de fichier, chemin, date)`.
pub fn list(dir: &Path) -> Vec<(String, PathBuf, u64)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "bgw") {
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let when = e
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            out.push((stem, p, when));
        }
    }
    out.sort_by(|a, b| b.2.cmp(&a.2));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_round_trip_and_corruption() {
        let mut s = SaveData {
            name: "test".into(),
            map_version: 2,
            seed: 12345,
            mode: 1,
            history: vec![[1, 2, 3, 255], [4, 5, 6, 8]],
            doors_open: vec![3, 9],
            sim: vec![1, 2, 3],
            ..Default::default()
        };
        s.players.insert(
            "Alex".into(),
            PlayerSave {
                pos: Vec3::new(1.0, 2.0, 3.0),
                hp: 80.0,
                weapon: 4,
                money: 500,
                earned: 900,
                slots: vec![[1, 0, 0], [1, 17, 51]],
            },
        );
        let bytes = s.encode();
        let back = SaveData::decode(&bytes).unwrap();
        assert_eq!(back.seed, 12345);
        assert_eq!(back.history, s.history);
        assert_eq!(back.doors_open, s.doors_open);
        assert_eq!(back.players["Alex"].money, 500);
        let mut bad = bytes.clone();
        bad[20] ^= 0xff;
        assert!(SaveData::decode(&bad).is_err());
    }
}
