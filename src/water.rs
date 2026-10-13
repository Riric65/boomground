//! Eau : automate cellulaire sur la grille des voxels, simulé par le serveur.
//!
//! Chaque cellule d'eau a un niveau de 1 à 8 (8 = pleine). À chaque pas,
//! l'eau tombe dans la cellule du dessous tant qu'elle n'est pas pleine,
//! puis s'égalise avec ses voisines horizontales : elle remplit les creux,
//! les sous-sols et les bunkers, déborde, s'écoule dans les escaliers.
//! Seules les cellules « actives » (qui ont bougé ou dont un voisin a bougé)
//! sont traitées, avec un budget par pas. Les nappes peu profondes
//! s'évaporent lentement pour qu'une inondation finisse par se retirer.
//!
//! Le client ne reçoit que les cellules **visibles** (niveau ≥ [`VISIBLE`])
//! et les affiche en maillage translucide.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::world::VoxelWorld;

/// Niveau d'une cellule pleine.
pub const FULL: u8 = 8;
/// Niveau à partir duquel une cellule est affichée.
pub const VISIBLE: u8 = 1;
/// Nombre maximal de cellules d'eau (mémoire, réseau).
pub const MAX_CELLS: usize = 300_000;
/// Cellules traitées par pas au plus.
const BUDGET: usize = 60_000;

const SIDES: [IVec3; 4] = [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z];

#[derive(Default)]
pub struct Water {
    pub cells: HashMap<IVec3, u8>,
    active: HashSet<IVec3>,
    /// Sources : cellule, débit (niveaux par pas), durée restante (s).
    sources: Vec<(IVec3, u8, f32)>,
    /// Cellules visibles connues des clients.
    shown: HashSet<IVec3>,
    /// Cellules modifiées depuis le dernier envoi aux clients.
    touched: HashSet<IVec3>,
    rng: u64,
    evaporate_in: f32,
}

impl Water {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn level(&self, c: IVec3) -> u8 {
        self.cells.get(&c).copied().unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    fn rand(&mut self) -> u32 {
        self.rng = self.rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.rng >> 33) as u32
    }

    /// Ajoute de l'eau dans une cellule libre (rien si elle est solide).
    pub fn add(&mut self, world: &VoxelWorld, c: IVec3, amount: u8) {
        if amount == 0 || world.is_solid(c) || !in_grid(world, c) {
            return;
        }
        if self.cells.len() >= MAX_CELLS && !self.cells.contains_key(&c) {
            return;
        }
        self.touched.insert(c);
        let l = self.cells.entry(c).or_insert(0);
        *l = (*l).saturating_add(amount).min(FULL);
        self.wake(c);
    }

    /// Retire l'eau d'une cellule.
    pub fn remove(&mut self, c: IVec3) {
        if { self.touched.insert(c); self.cells.remove(&c) }.is_some() {
            self.wake(c);
            self.wake(c - IVec3::Y);
        }
    }

    /// Vide toute l'eau ; renvoie les cellules qui étaient affichées.
    pub fn drain_all(&mut self) -> Vec<IVec3> {
        let shown = self.shown.drain().collect();
        self.touched.clear();
        self.cells.clear();
        self.active.clear();
        self.sources.clear();
        shown
    }

    /// Source d'eau (bouche d'incendie cassée, vague…).
    pub fn add_source(&mut self, c: IVec3, rate: u8, seconds: f32) {
        if self.sources.len() < 64 {
            self.sources.push((c, rate, seconds));
        }
    }

    /// Réveille une cellule et ses voisines (un voxel a changé à côté).
    pub fn wake(&mut self, c: IVec3) {
        self.active.insert(c);
        self.active.insert(c + IVec3::Y);
        for s in SIDES {
            self.active.insert(c + s);
        }
    }

    /// Un voxel a disparu : l'eau voisine peut s'y engouffrer.
    pub fn wake_around(&mut self, cells: &[IVec3]) {
        if self.cells.is_empty() {
            return;
        }
        for &c in cells {
            for o in [IVec3::Y, IVec3::NEG_Y, IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z] {
                if self.cells.contains_key(&(c + o)) {
                    self.active.insert(c + o);
                }
            }
        }
    }

    /// Un pas de simulation. `dt` en secondes (le serveur appelle ~10 fois
    /// par seconde).
    pub fn step(&mut self, world: &VoxelWorld, dt: f32) {
        // Sources.
        let mut i = 0;
        while i < self.sources.len() {
            let (c, rate, ref mut left) = self.sources[i];
            *left -= dt;
            if *left <= 0.0 {
                self.sources.swap_remove(i);
                continue;
            }
            self.add(world, c, rate);
            i += 1;
        }

        // Cellules actives, du bas vers le haut (l'eau du bas descend
        // d'abord, celle du dessus suit dans le même pas).
        let mut todo: Vec<IVec3> = self.active.drain().filter(|c| self.cells.contains_key(c)).collect();
        if todo.len() > BUDGET {
            // Le reste attend le pas suivant.
            for c in todo.drain(BUDGET..) {
                self.active.insert(c);
            }
        }
        todo.sort_unstable_by_key(|c| c.y);
        for c in todo {
            let Some(&lvl) = self.cells.get(&c) else { continue };
            if lvl == 0 {
                { self.touched.insert(c); self.cells.remove(&c) };
                continue;
            }
            // Cellule devenue solide (construction) : l'eau est chassée vers
            // le haut.
            if world.is_solid(c) {
                { self.touched.insert(c); self.cells.remove(&c) };
                self.add(world, c + IVec3::Y, lvl);
                continue;
            }
            let mut lvl = lvl;
            // 1. Chute.
            let below = c - IVec3::Y;
            if below.y >= 1 && !world.is_solid(below) {
                let lb = self.level(below);
                if lb < FULL {
                    let moved = lvl.min(FULL - lb);
                    self.touch(below); self.cells.insert(below, lb + moved);
                    lvl -= moved;
                    self.wake(below);
                }
            }
            // 2. Étalement : on donne aux voisines plus basses, dans un ordre
            // tiré au sort (sinon l'eau glisse toujours du même côté).
            if lvl > 1 {
                let start = (self.rand() % 4) as usize;
                for k in 0..4 {
                    if lvl <= 1 {
                        break;
                    }
                    let n = c + SIDES[(start + k) % 4];
                    if world.is_solid(n) || !in_grid(world, n) {
                        continue;
                    }
                    let ln = self.level(n);
                    if ln + 1 < lvl {
                        if ln == 0 && self.cells.len() >= MAX_CELLS {
                            continue;
                        }
                        let give = ((lvl - ln) / 2).max(1);
                        self.touch(n); self.cells.insert(n, ln + give);
                        lvl -= give;
                        self.wake(n);
                    }
                }
            }
            if lvl == 0 {
                { self.touched.insert(c); self.cells.remove(&c) };
                self.wake(c);
            } else if lvl != self.cells[&c] {
                self.touch(c); self.cells.insert(c, lvl);
                self.wake(c);
            }
        }

        // Évaporation lente des nappes peu profondes.
        self.evaporate_in -= dt;
        if self.evaporate_in <= 0.0 {
            self.evaporate_in = 2.0;
            let keys: Vec<IVec3> = self.cells.keys().copied().collect();
            for c in keys {
                let l = self.cells[&c];
                // Une cellule couverte d'eau (dans un bassin) ne s'évapore pas.
                if l < FULL && !self.cells.contains_key(&(c + IVec3::Y)) && self.rand() % 100 < 4 {
                    if l <= 1 {
                        { self.touched.insert(c); self.cells.remove(&c) };
                    } else {
                        self.touch(c); self.cells.insert(c, l - 1);
                    }
                    self.wake(c);
                }
            }
        }
    }

    /// Différences d'affichage depuis le dernier appel : (apparues,
    /// disparues). Seules les cellules modifiées sont examinées.
    pub fn visible_delta(&mut self) -> (Vec<IVec3>, Vec<IVec3>) {
        let mut added = Vec::new();
        let mut removed = Vec::new();
        for c in std::mem::take(&mut self.touched) {
            let vis = self.cells.get(&c).is_some_and(|l| *l >= VISIBLE);
            if vis && self.shown.insert(c) {
                added.push(c);
            } else if !vis && self.shown.remove(&c) {
                removed.push(c);
            }
        }
        (added, removed)
    }

    fn touch(&mut self, c: IVec3) {
        self.touched.insert(c);
    }

    /// Toutes les cellules visibles (pour un joueur qui arrive).
    pub fn visible(&self) -> Vec<IVec3> {
        self.shown.iter().copied().collect()
    }
}

fn in_grid(world: &VoxelWorld, c: IVec3) -> bool {
    c.cmpge(IVec3::ZERO).all() && c.cmplt(world.dims).all()
}

/// Eau vue par le client : cellules visibles, maillées comme des voxels.
#[derive(Resource, Default)]
pub struct WaterMirror {
    pub cells: HashSet<IVec3>,
    pub dirty: HashSet<IVec3>,
}

impl WaterMirror {
    pub fn apply(&mut self, add: &[IVec3], remove: &[IVec3]) {
        for &c in add {
            if self.cells.insert(c) {
                self.dirty.insert(c.div_euclid(IVec3::splat(crate::vmesh::CHUNK)));
            }
        }
        for c in remove {
            if self.cells.remove(c) {
                self.dirty.insert(c.div_euclid(IVec3::splat(crate::vmesh::CHUNK)));
            }
        }
    }

    pub fn contains(&self, c: IVec3) -> bool {
        self.cells.contains(&c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::mat;

    /// Un bassin : l'eau versée tombe, puis remplit le fond sans déborder.
    #[test]
    fn water_falls_and_fills_a_basin() {
        let mut w = VoxelWorld::with_blocks(5);
        w.fill(IVec3::new(10, 1, 10), IVec3::new(20, 2, 20), mat::CONCRETE);
        for (a, b) in [
            (IVec3::new(10, 2, 10), IVec3::new(20, 6, 11)),
            (IVec3::new(10, 2, 19), IVec3::new(20, 6, 20)),
            (IVec3::new(10, 2, 10), IVec3::new(11, 6, 20)),
            (IVec3::new(19, 2, 10), IVec3::new(20, 6, 20)),
        ] {
            w.fill(a, b, mat::CONCRETE);
        }
        let mut water = Water::default();
        for _ in 0..40 {
            water.add(&w, IVec3::new(15, 12, 15), FULL);
            water.step(&w, 0.1);
        }
        for _ in 0..400 {
            water.step(&w, 0.1);
        }
        // Le fond du bassin (y = 2) est rempli partout, rien n'est resté en l'air.
        let bottom = (11..19).flat_map(|x| (11..19).map(move |z| IVec3::new(x, 2, z))).filter(|c| water.level(*c) > 0).count();
        assert!(bottom >= 60, "fond du bassin rempli : {bottom}/64");
        assert!(water.cells.keys().all(|c| c.y < 6), "rien ne flotte au-dessus");
    }
}
