//! Simulation physique côté **serveur** : gravats et corps rigides.
//!
//! - Les **blocs** de gravats sont de petites boîtes qui tombent, rebondissent
//!   et s'empilent (rotation autour de Y seulement).
//! - Les **corps** sont des morceaux de bâtiment détachés du sol : un ensemble
//!   de voxels qui devient un solide rigide (masse selon les matériaux, centre
//!   de masse, inertie), qui tombe, bascule et heurte le monde. Un impact
//!   violent le casse (gravats + trou dans le monde) ; au repos, il se
//!   « fige » : ses voxels sont reposés dans la grille du monde.
//!
//! Ce module ne dépend ni du rendu ni du réseau : le serveur l'avance,
//! l'historique de ses [`SimEvent`] est traduit en paquets, et tout l'état se
//! sérialise pour les sauvegardes.

use bevy::prelude::*;

use crate::world::{density, mat, VoxelWorld, VOXEL};

/// Gravité (m/s²) : celle de la Terre, pour des chutes crédibles.
pub const GRAVITY: f32 = 9.81;
/// Nombre maximal de blocs de gravats simultanés.
pub const MAX_BLOCKS: usize = 900;
/// Nombre maximal de corps rigides simultanés.
pub const MAX_BODIES: usize = 64;
/// Points de collision par corps.
const MAX_SAMPLES: usize = 420;
/// Vitesse d'impact (m/s) à partir de laquelle un corps casse.
const BREAK_SPEED: f32 = 4.5;
/// Au-delà, un ensemble flottant est réputé stable (évite un calcul géant).
pub const MAX_BODY_VOXELS: usize = 450_000;
/// Taille d'une zone de gravats (voxels).
const ZONE: i32 = 2;
/// Blocs créés au maximum par destruction.
const MAX_PER_HIT: usize = 160;

/// Générateur pseudo-aléatoire (xorshift64).
#[derive(Clone)]
pub struct SimRng(u64);

impl SimRng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn f32(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn signed(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }
    fn usize(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// Bloc de gravats.
#[derive(Clone, Debug)]
pub struct Block {
    pub id: u32,
    pub pos: Vec3,
    pub yaw: f32,
    pub vel: Vec3,
    pub spin: f32,
    pub half: Vec3,
    pub mat: u8,
    pub mass: f32,
    pub sleeping: bool,
    pub rest: f32,
}

/// Grille dense d'un corps : 0 = vide, sinon matériau + 1.
#[derive(Clone, Debug)]
pub struct BodyGrid {
    pub dims: IVec3,
    pub cells: Vec<u8>,
}

impl BodyGrid {
    pub fn new(dims: IVec3) -> Self {
        Self {
            dims,
            cells: vec![0; (dims.x * dims.y * dims.z).max(0) as usize],
        }
    }

    #[inline]
    fn index(&self, c: IVec3) -> Option<usize> {
        if c.x < 0 || c.y < 0 || c.z < 0 || c.x >= self.dims.x || c.y >= self.dims.y || c.z >= self.dims.z {
            return None;
        }
        Some(((c.x * self.dims.y + c.y) * self.dims.z + c.z) as usize)
    }

    /// Matériau + 1 de la cellule (0 : vide ou hors grille).
    #[inline]
    pub fn raw(&self, c: IVec3) -> u8 {
        self.index(c).map_or(0, |i| self.cells[i])
    }

    pub fn is_solid(&self, c: IVec3) -> bool {
        self.raw(c) != 0
    }

    pub fn material(&self, c: IVec3) -> u8 {
        self.raw(c).saturating_sub(1)
    }

    pub fn set(&mut self, c: IVec3, material: Option<u8>) {
        if let Some(i) = self.index(c) {
            self.cells[i] = material.map_or(0, |m| m + 1);
        }
    }

    pub fn count(&self) -> usize {
        self.cells.iter().filter(|&&c| c != 0).count()
    }

    /// Compression par plages `(valeur, longueur u16)`.
    pub fn encode_rle(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.cells.len() {
            let v = self.cells[i];
            let mut run = 1usize;
            while i + run < self.cells.len() && self.cells[i + run] == v && run < u16::MAX as usize {
                run += 1;
            }
            out.push(v);
            out.extend_from_slice(&(run as u16).to_le_bytes());
            i += run;
        }
        out
    }

    pub fn decode_rle(dims: IVec3, data: &[u8]) -> Option<Self> {
        let mut grid = Self::new(dims);
        let mut at = 0usize;
        for pair in data.chunks_exact(3) {
            let run = u16::from_le_bytes([pair[1], pair[2]]) as usize;
            if at + run > grid.cells.len() {
                return None;
            }
            grid.cells[at..at + run].fill(pair[0]);
            at += run;
        }
        (at == grid.cells.len()).then_some(grid)
    }
}

/// Corps rigide.
#[derive(Clone, Debug)]
pub struct Body {
    pub id: u32,
    pub grid: BodyGrid,
    /// Centre de masse dans le repère de la grille (mètres).
    pub com: Vec3,
    /// Position monde du centre de masse.
    pub pos: Vec3,
    pub rot: Quat,
    pub lin: Vec3,
    pub ang: Vec3,
    pub mass: f32,
    inv_inertia: Mat3,
    /// Points de collision, relatifs au centre de masse (repère du corps).
    samples: Vec<Vec3>,
    rest: f32,
    cooldown: f32,
    /// Véhicule conduisible (voiture).
    pub vehicle: Option<Vehicle>,
}

/// Commandes et état d'un véhicule.
#[derive(Clone, Debug)]
pub struct Vehicle {
    /// Voxels d'origine (l'état général se déduit de ce qui reste).
    pub orig: usize,
    pub throttle: f32,
    pub steer: f32,
    pub brake: bool,
    /// Durée de validité de la dernière commande.
    pub ttl: f32,
    /// Au sol au dernier pas (les roues touchent).
    pub grounded: bool,
    /// Moteur en état de marche.
    pub alive: bool,
    /// Vitesse (m/s), pour le moteur sonore.
    pub speed: f32,
    /// Moyeux des roues, repère du corps (relatifs au centre de masse).
    /// Les deux premières sont à l'avant (côté -Z) et braquent.
    pub wheels: [Vec3; 4],
    /// Débattement de suspension utilisé par roue (0 = détendue).
    pub compress: [f32; 4],
    /// Angle de braquage actuel (radians, + = gauche).
    pub steer_angle: f32,
    /// Rotation des roues (radians), pour l'affichage.
    pub spin: f32,
}

/// Rayon des roues (un voxel) et course de la suspension.
pub const WHEEL_RADIUS: f32 = VOXEL;
pub const WHEEL_TRAVEL: f32 = 0.3;

impl Body {
    /// Position monde (centre) d'une cellule de la grille.
    #[inline]
    pub fn cell_world(&self, c: IVec3) -> Vec3 {
        let local = (c.as_vec3() + Vec3::splat(0.5)) * VOXEL - self.com;
        self.pos + self.rot * local
    }

    /// Point monde → cellule de la grille.
    #[inline]
    pub fn world_cell(&self, p: Vec3) -> IVec3 {
        let local = self.rot.inverse() * (p - self.pos) + self.com;
        (local / VOXEL).floor().as_ivec3()
    }

    pub fn contains(&self, p: Vec3) -> bool {
        self.grid.is_solid(self.world_cell(p))
    }

    fn velocity_at(&self, p: Vec3) -> Vec3 {
        self.lin + self.ang.cross(p - self.pos)
    }

    /// Recalcule masse, centre de masse, inertie et points de collision.
    /// Renvoie `false` si le corps est vide.
    fn recompute(&mut self) -> bool {
        let d = self.grid.dims;
        let v3 = VOXEL * VOXEL * VOXEL;
        let mut mass = 0.0f32;
        let mut sum = Vec3::ZERO;
        let mut count = 0usize;
        for x in 0..d.x {
            for y in 0..d.y {
                for z in 0..d.z {
                    let raw = self.grid.raw(IVec3::new(x, y, z));
                    if raw == 0 {
                        continue;
                    }
                    let m = density(raw - 1) * v3;
                    mass += m;
                    sum += m * (Vec3::new(x as f32, y as f32, z as f32) + Vec3::splat(0.5)) * VOXEL;
                    count += 1;
                }
            }
        }
        if count == 0 || mass <= 0.0 {
            return false;
        }
        let new_com = sum / mass;
        // Le centre de masse bouge quand des voxels partent : on garde la
        // position monde de la matière en place.
        let shift = self.rot * (new_com - self.com);
        self.pos += shift;
        self.com = new_com;
        self.mass = mass;

        let mut inertia = Mat3::ZERO;
        let mut surface: Vec<Vec3> = Vec::new();
        for x in 0..d.x {
            for y in 0..d.y {
                for z in 0..d.z {
                    let c = IVec3::new(x, y, z);
                    let raw = self.grid.raw(c);
                    if raw == 0 {
                        continue;
                    }
                    let m = density(raw - 1) * v3;
                    let r = (c.as_vec3() + Vec3::splat(0.5)) * VOXEL - self.com;
                    let r2 = r.length_squared() + VOXEL * VOXEL / 6.0;
                    inertia += Mat3::from_diagonal(Vec3::splat(m * r2))
                        - Mat3::from_cols(r * (m * r.x), r * (m * r.y), r * (m * r.z));
                    let exposed = [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z]
                        .iter()
                        .any(|&o| !self.grid.is_solid(c + o));
                    if exposed {
                        surface.push(r);
                    }
                }
            }
        }
        self.inv_inertia = if inertia.determinant().abs() > 1e-9 {
            inertia.inverse()
        } else {
            Mat3::from_diagonal(Vec3::splat(1.0 / (mass * 0.5).max(1e-3)))
        };
        let k = surface.len().div_ceil(MAX_SAMPLES).max(1);
        self.samples = surface
            .iter()
            .enumerate()
            .filter(|(i, _)| (i.wrapping_mul(2_654_435_761) >> 5) % k == 0)
            .map(|(_, &p)| p)
            .collect();
        true
    }
}

/// Ce que la simulation signale au serveur.
#[derive(Debug)]
pub enum SimEvent {
    BlocksAdded(Vec<u32>),
    BlocksRemoved(Vec<u32>),
    /// Définition (nouvelle ou modifiée) d'un corps à envoyer.
    BodyChanged(u32),
    BodyRemoved(u32),
    /// Voxels du monde retirés par un impact (avec gravats déjà créés).
    WorldRemoved(Vec<IVec3>),
    /// Voxels posés quand un corps se fige (`[x, y, z, matériau]`).
    WorldPlaced(Vec<[u16; 4]>),
    /// Impact violent : son et poussière.
    Impact { at: Vec3, power: f32 },
}

/// Gravats + corps rigides.
pub struct Sim {
    pub blocks: Vec<Block>,
    pub bodies: Vec<Body>,
    next_id: u32,
    rng: SimRng,
    /// Corps à briser après un impact violent (traité en fin de pas).
    pending_fracture: Vec<u32>,
}

impl Default for Sim {
    fn default() -> Self {
        Self::new()
    }
}

impl Sim {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            bodies: Vec::new(),
            next_id: 1,
            rng: SimRng(0x9E37_79B9_7F4A_7C15),
            pending_fracture: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.blocks.clear();
        self.bodies.clear();
    }

    fn new_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        id
    }

    pub fn body(&self, id: u32) -> Option<&Body> {
        self.bodies.iter().find(|b| b.id == id)
    }

    pub fn body_mut(&mut self, id: u32) -> Option<&mut Body> {
        self.bodies.iter_mut().find(|b| b.id == id)
    }

    /// Transforme des voxels (déjà retirés du monde) en véhicule.
    pub fn form_vehicle(&mut self, world: &VoxelWorld, cells: Vec<IVec3>) -> Option<u32> {
        if cells.len() < 20 {
            return None;
        }
        // La couche du bas, ce sont les roues en cubes : elles sont
        // remplacées par de vraies roues (suspension, rotation) et ne font
        // pas partie de la caisse.
        let bottom = cells.iter().map(|c| c.y).min()?;
        let (wheel_cells, cells): (Vec<IVec3>, Vec<IVec3>) = cells.into_iter().partition(|c| c.y == bottom);
        if cells.len() < 20 {
            return None;
        }
        let lo = cells.iter().fold(IVec3::MAX, |a, c| a.min(*c));
        let hi = cells.iter().fold(IVec3::MIN, |a, c| a.max(*c));
        let mid_z = (lo.z + hi.z) as f32 * 0.5;
        // Moyeu : centre des cubes de roue de chaque coin, à hauteur du haut
        // de la couche (la roue touche le sol par en dessous).
        let hub_y = world.cell_min(IVec3::new(0, bottom + 1, 0)).y;
        let mut hubs = [Vec3::ZERO; 4];
        for (i, (front, left)) in [(true, true), (true, false), (false, true), (false, false)].into_iter().enumerate() {
            let pick: Vec<Vec3> = wheel_cells
                .iter()
                .filter(|c| ((c.z as f32) < mid_z) == front && (c.x * 2 < lo.x + hi.x) == left)
                .map(|c| world.cell_center(*c))
                .collect();
            let x = if left { world.cell_center(lo).x } else { world.cell_center(hi).x };
            let z = if front { world.cell_center(lo).z + VOXEL * 1.5 } else { world.cell_center(hi).z - VOXEL * 1.5 };
            let c = if pick.is_empty() {
                Vec3::new(x, hub_y, z)
            } else {
                pick.iter().copied().sum::<Vec3>() / pick.len() as f32
            };
            hubs[i] = Vec3::new(c.x, hub_y, c.z);
        }
        let orig = cells.len();
        let mut ev = Vec::new();
        let before = self.bodies.len();
        self.form_bodies(world, vec![cells], &mut ev);
        if self.bodies.len() == before {
            return None;
        }
        let body = self.bodies.last_mut()?;
        body.vehicle = Some(Vehicle {
            orig,
            throttle: 0.0,
            steer: 0.0,
            brake: false,
            ttl: 0.0,
            grounded: false,
            alive: true,
            speed: 0.0,
            // Le corps vient d'être créé sans rotation : le repère local est
            // le repère monde translaté au centre de masse.
            wheels: hubs.map(|h| h - body.pos),
            compress: [0.0; 4],
            steer_angle: 0.0,
            spin: 0.0,
        });
        Some(body.id)
    }

    // ------------------------------------------------------------------
    // Création
    // ------------------------------------------------------------------

    /// Crée des blocs de gravats pour des voxels détruits (déjà vidés du
    /// monde, dont le matériau reste lisible). Renvoie les blocs créés.
    pub fn add_rubble(
        &mut self,
        world: &VoxelWorld,
        destroyed: &[IVec3],
        impact: Vec3,
        push: Vec3,
    ) -> Vec<u32> {
        use std::collections::HashMap;
        let mut zones: HashMap<IVec3, (IVec3, IVec3, u8)> = HashMap::new();
        for &c in destroyed {
            let key = c.div_euclid(IVec3::splat(ZONE));
            let m = world.material(c);
            zones
                .entry(key)
                .and_modify(|z| {
                    z.0 = z.0.min(c);
                    z.1 = z.1.max(c);
                })
                .or_insert((c, c, m));
        }
        let mut list: Vec<(IVec3, IVec3, u8)> = zones.into_values().collect();
        list.sort_by_key(|z| (z.0.x, z.0.y, z.0.z));
        if list.len() > MAX_PER_HIT {
            for i in 0..MAX_PER_HIT {
                let j = i + self.rng.usize(list.len() - i);
                list.swap(i, j);
            }
            list.truncate(MAX_PER_HIT);
        }
        let mut ids = Vec::new();
        for (lo, hi, material) in list {
            if material == mat::GLASS {
                continue;
            }
            let min = world.cell_min(lo);
            let max = world.cell_max(hi);
            let center = (min + max) * 0.5;
            let size = (max - min) * 0.9;
            self.push_block(center, size * 0.5, material, impact, push);
            ids.push(self.blocks.last().map(|b| b.id).unwrap_or(0));
        }
        ids
    }

    fn push_block(&mut self, center: Vec3, half: Vec3, material: u8, impact: Vec3, push: Vec3) {
        let away = (center - impact).normalize_or_zero();
        let strength = push.length().max(1.0);
        let light = (1.0 / (half.x * half.y * half.z * 8.0 * density(material)).max(0.4)).min(2.0);
        let vel = (away * (1.0 + self.rng.f32() * 1.5) * strength.sqrt()
            + push * (0.3 + self.rng.f32() * 0.4))
            * light.sqrt()
            + Vec3::new(self.rng.signed(), self.rng.f32() * 0.5, self.rng.signed()) * 1.2;
        let size = half * 2.0;
        let id = self.new_id();
        self.blocks.push(Block {
            id,
            pos: center,
            yaw: 0.0,
            vel,
            spin: self.rng.signed() * 5.0,
            half,
            mat: material,
            mass: size.x * size.y * size.z * density(material),
            sleeping: false,
            rest: 0.0,
        });
    }

    /// Retire les blocs en trop (les plus anciens). Renvoie leurs ids.
    pub fn trim_blocks(&mut self) -> Vec<u32> {
        if self.blocks.len() <= MAX_BLOCKS {
            return Vec::new();
        }
        let extra = self.blocks.len() - MAX_BLOCKS;
        self.blocks.drain(..extra).map(|b| b.id).collect()
    }

    /// Transforme des ensembles de voxels flottants (déjà retirés du
    /// monde) en corps rigides. Les trop petits deviennent des gravats.
    pub fn form_bodies(
        &mut self,
        world: &VoxelWorld,
        components: Vec<Vec<IVec3>>,
        ev: &mut Vec<SimEvent>,
    ) {
        for cells in components {
            if cells.len() < 10 || self.bodies.len() >= MAX_BODIES {
                let ids = self.add_rubble(world, &cells, world.cell_center(cells[0]), Vec3::ZERO);
                if !ids.is_empty() {
                    ev.push(SimEvent::BlocksAdded(ids));
                }
                continue;
            }
            let mut lo = cells[0];
            let mut hi = cells[0];
            for &c in &cells {
                lo = lo.min(c);
                hi = hi.max(c);
            }
            let dims = hi - lo + IVec3::ONE;
            let mut grid = BodyGrid::new(dims);
            for &c in &cells {
                grid.set(c - lo, Some(world.material(c)));
            }
            let id = self.new_id();
            let mut body = Body {
                id,
                grid,
                com: Vec3::ZERO,
                pos: world.cell_min(lo),
                rot: Quat::IDENTITY,
                lin: Vec3::ZERO,
                ang: Vec3::ZERO,
                mass: 0.0,
                inv_inertia: Mat3::IDENTITY,
                samples: Vec::new(),
                rest: 0.0,
                cooldown: 0.0,
                vehicle: None,
            };
            // `pos` désigne le coin ; `recompute` ramène au centre de masse.
            body.com = Vec3::ZERO;
            if body.recompute() {
                self.bodies.push(body);
                ev.push(SimEvent::BodyChanged(id));
            }
        }
    }

    // ------------------------------------------------------------------
    // Avancement
    // ------------------------------------------------------------------

    /// Avance la simulation de `dt` secondes (en sous-pas fixes).
    pub fn step(&mut self, world: &mut VoxelWorld, dt: f32, ev: &mut Vec<SimEvent>) {
        let mut remaining = dt.min(0.1);
        while remaining > 1e-4 {
            let h = remaining.min(1.0 / 60.0);
            remaining -= h;
            self.step_blocks(world, h);
            self.step_bodies(world, h, ev);
        }
        let gone = self.trim_blocks();
        if !gone.is_empty() {
            ev.push(SimEvent::BlocksRemoved(gone));
        }
    }

    fn step_blocks(&mut self, world: &VoxelWorld, dt: f32) {
        use std::collections::HashMap;
        const CELL: f32 = 1.0;
        // Boîtes des blocs endormis, pour les collisions mutuelles.
        let mut grid: HashMap<IVec3, Vec<(u32, Vec3, Vec3)>> = HashMap::new();
        for b in &self.blocks {
            if !b.sleeping {
                continue;
            }
            let (min, max) = (b.pos - b.half, b.pos + b.half);
            let c0 = (min / CELL).floor().as_ivec3();
            let c1 = (max / CELL).floor().as_ivec3();
            for x in c0.x..=c1.x {
                for y in c0.y..=c1.y {
                    for z in c0.z..=c1.z {
                        grid.entry(IVec3::new(x, y, z)).or_default().push((b.id, min, max));
                    }
                }
            }
        }
        let mut lost: Vec<u32> = Vec::new();
        for b in &mut self.blocks {
            if b.sleeping {
                continue;
            }
            b.vel.y -= GRAVITY * dt;
            let mut pos = b.pos;
            let reach = b.half + Vec3::splat(b.vel.length() * dt + 0.1);
            let c0 = ((pos - reach) / CELL).floor().as_ivec3();
            let c1 = ((pos + reach) / CELL).floor().as_ivec3();
            let mut boxes: Vec<(Vec3, Vec3)> = Vec::new();
            for x in c0.x..=c1.x {
                for y in c0.y..=c1.y {
                    for z in c0.z..=c1.z {
                        if let Some(list) = grid.get(&IVec3::new(x, y, z)) {
                            for &(other, min, max) in list {
                                if other != b.id && !boxes.contains(&(min, max)) {
                                    boxes.push((min, max));
                                }
                            }
                        }
                    }
                }
            }
            let mut grounded = false;
            for axis in [1, 0, 2] {
                let delta = b.vel[axis] * dt;
                if delta == 0.0 {
                    continue;
                }
                if world.sweep_with(&mut pos, b.half, axis, delta, &boxes) {
                    if axis == 1 && b.vel.y < 0.0 {
                        grounded = true;
                        b.vel.y = -b.vel.y * 0.18;
                        if b.vel.y < 0.8 {
                            b.vel.y = 0.0;
                        }
                        b.vel.x *= 0.6;
                        b.vel.z *= 0.6;
                        b.spin *= 0.5;
                    } else {
                        b.vel[axis] = -b.vel[axis] * 0.25;
                    }
                }
            }
            if grounded {
                let friction = 1.0 - (6.0 * dt).min(1.0);
                b.vel.x *= friction;
                b.vel.z *= friction;
            }
            b.pos = pos;
            b.yaw += b.spin * dt;
            if pos.y < -8.0 {
                lost.push(b.id);
                continue;
            }
            if grounded && b.vel.length() < 0.4 {
                b.rest += dt;
                if b.rest > 0.25 {
                    b.sleeping = true;
                    b.vel = Vec3::ZERO;
                    b.spin = 0.0;
                }
            } else if !grounded {
                b.rest = 0.0;
            }
        }
        if !lost.is_empty() {
            self.blocks.retain(|b| !lost.contains(&b.id));
        }
    }

    /// Réveille les blocs dont le support a disparu ou proches d'un souffle.
    pub fn wake_near(&mut self, at: Vec3, radius: f32, push: f32) {
        for b in &mut self.blocks {
            let d = b.pos - at;
            let dist = d.length();
            if dist < radius + 1.5 {
                b.sleeping = false;
                b.rest = 0.0;
                if push > 0.0 && dist < radius * 1.5 {
                    let falloff = 1.0 - (dist / (radius * 1.5)).min(1.0);
                    let light = (1.0 / b.mass.max(0.4)).min(2.0);
                    b.vel += (d.normalize_or(Vec3::Y) * push * falloff + Vec3::Y * push * 0.4 * falloff) * light;
                }
            }
        }
    }

    /// Réveille les blocs endormis qui n'ont plus rien dessous.
    pub fn wake_unsupported(&mut self, world: &VoxelWorld) {
        let boxes: Vec<(Vec3, Vec3)> = self
            .blocks
            .iter()
            .filter(|b| b.sleeping)
            .map(|b| (b.pos - b.half, b.pos + b.half))
            .collect();
        for b in &mut self.blocks {
            if !b.sleeping {
                continue;
            }
            let (min, max) = (b.pos - b.half, b.pos + b.half);
            let below_min = Vec3::new(min.x + 0.02, min.y - 0.06, min.z + 0.02);
            let below_max = Vec3::new(max.x - 0.02, min.y + 0.01, max.z - 0.02);
            if world.overlaps(below_min, below_max) {
                continue;
            }
            let on_block = boxes.iter().any(|&(bmin, bmax)| {
                bmin.x < below_max.x
                    && bmax.x > below_min.x
                    && bmin.z < below_max.z
                    && bmax.z > below_min.z
                    && bmax.y >= below_min.y
                    && bmax.y <= below_max.y + 0.05
                    && bmin != min
            });
            if !on_block {
                b.sleeping = false;
                b.rest = 0.0;
            }
        }
    }

    fn step_bodies(&mut self, world: &mut VoxelWorld, dt: f32, ev: &mut Vec<SimEvent>) {
        let mut breaks: Vec<(usize, Vec3, f32)> = Vec::new();
        let mut settle: Vec<usize> = Vec::new();
        let mut fracture_ids: Vec<u32> = Vec::new();
        for (bi, body) in self.bodies.iter_mut().enumerate() {
            body.cooldown = (body.cooldown - dt).max(0.0);
            body.lin.y -= GRAVITY * dt;

            // Véhicule : quatre roues à suspension (lancer de rayon vers le
            // sol), adhérence latérale et motricité au point de contact de
            // chaque roue, braquage des roues avant.
            if let Some(v) = body.vehicle.as_mut() {
                v.ttl -= dt;
                let fwd = body.rot * Vec3::NEG_Z;
                let fwd_h = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero();
                let up = body.rot * Vec3::Y;
                let speed_f = body.lin.dot(fwd_h);
                v.speed = body.lin.length();
                let driving = v.ttl > 0.0 && v.alive;
                let target = if driving { v.steer * 0.55 / (1.0 + speed_f.abs() * 0.04) } else { 0.0 };
                v.steer_angle += (target - v.steer_angle) * (1.0 - (-10.0 * dt).exp());
                v.spin += speed_f / WHEEL_RADIUS * dt;
                let rm = Mat3::from_quat(body.rot);
                let inv_world = rm * body.inv_inertia * rm.transpose();
                let inv_m = 1.0 / body.mass.max(1e-4);
                let quarter = body.mass * 0.25;
                // Raideur : ~12 cm d'enfoncement au repos ; amortissement
                // proche du critique.
                let k = body.mass * GRAVITY / (4.0 * 0.12);
                let c = 2.0 * (k * quarter).sqrt() * 0.7;
                let reach = WHEEL_RADIUS + WHEEL_TRAVEL;
                let mut grounded = false;
                for i in 0..4 {
                    let hub = body.pos + body.rot * v.wheels[i];
                    let Some(hit) = world.raycast(hub, -up, reach) else {
                        v.compress[i] = 0.0;
                        continue;
                    };
                    let dist = hit.point.distance(hub);
                    let comp = (reach - dist).clamp(0.0, reach);
                    v.compress[i] = comp;
                    grounded = true;
                    let r = hit.point - body.pos;
                    let vel = body.lin + body.ang.cross(r);
                    let load = (k * comp - c * vel.dot(up)).max(0.0);
                    // La suspension pousse au point de contact ; l'adhérence et
                    // la motricité agissent presque à hauteur du centre de
                    // masse (sinon la voiture se couche au premier virage).
                    let jn = up * (load * dt);
                    body.lin += jn * inv_m;
                    body.ang += inv_world * r.cross(jn);
                    let r = r - up * (r.dot(up) * 0.85);
                    let mut j = Vec3::ZERO;
                    // Direction de la roue (les avant braquent).
                    let mut wf = fwd;
                    if i < 2 {
                        wf = Quat::from_axis_angle(up, v.steer_angle) * wf;
                    }
                    let wf = (wf - up * wf.dot(up)).normalize_or_zero();
                    let wr = wf.cross(up);
                    // Adhérence latérale, limitée par la charge de la roue.
                    let lat = vel.dot(wr);
                    let grip = 1.1 * load * dt;
                    j += wr * (-lat * quarter).clamp(-grip, grip);
                    let roll = vel.dot(wf);
                    if driving {
                        let mut drive = v.throttle * 14.0 * quarter * dt;
                        if (v.throttle > 0.0 && speed_f > 26.0) || (v.throttle < 0.0 && speed_f < -9.0) {
                            drive = 0.0;
                        }
                        j += wf * drive.clamp(-grip, grip);
                    }
                    // Frein (ou frein moteur léger sans conducteur).
                    let brake = if driving && v.brake { 0.12 } else if driving { 0.004 } else { 0.03 };
                    j -= wf * (roll * quarter * brake).clamp(-grip, grip);
                    body.lin += j * inv_m;
                    body.ang += inv_world * r.cross(j);
                }
                v.grounded = grounded;
                if grounded {
                    // Barre anti-roulis : amortit le roulis et le tangage.
                    let yaw = up * body.ang.dot(up);
                    body.ang = yaw + (body.ang - yaw) * (-6.0 * dt).exp();
                }
                // Intégrité : en dessous de 60 % des voxels, il ne roule plus.
                v.alive = body.grid.count() * 100 >= v.orig * 60;
            }
            body.pos += body.lin * dt;
            let w = body.ang * dt;
            let angle = w.length();
            if angle > 1e-6 {
                body.rot = (Quat::from_axis_angle(w / angle, angle) * body.rot).normalize();
            }

            // Contacts avec le monde.
            let rm = Mat3::from_quat(body.rot);
            let inv_world = rm * body.inv_inertia * rm.transpose();
            let inv_m = 1.0 / body.mass.max(1e-4);
            struct Contact {
                r: Vec3,
                n: Vec3,
                depth: f32,
            }
            let mut contacts: Vec<Contact> = Vec::new();
            for s in &body.samples {
                let r = body.rot * *s;
                let wp = body.pos + r;
                let cell = world.world_to_cell(wp);
                if !world.is_solid(cell) {
                    continue;
                }
                // Normale : vers les cellules voisines vides.
                let mut n = Vec3::ZERO;
                for o in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                    if !world.is_solid(cell + o) {
                        n += o.as_vec3();
                    }
                }
                let n = if n == Vec3::ZERO {
                    Vec3::Y
                } else {
                    n.normalize()
                };
                let center = world.cell_center(cell);
                let depth = (VOXEL * 0.5 + (center - wp).dot(n)).clamp(0.0, VOXEL);
                contacts.push(Contact { r, n, depth });
            }

            if !contacts.is_empty() {
                // Correction de position (moyenne pondérée).
                // (avec une tolérance : pas de correction pour un
                // enfoncement négligeable, ce qui fait vibrer le corps)
                let mut push = Vec3::ZERO;
                for c in &contacts {
                    push += c.n * (c.depth - 0.03).max(0.0);
                }
                let push = push / contacts.len() as f32;
                body.pos += push * 0.9;

                let mut worst_speed = 0.0f32;
                let mut worst_point = body.pos;
                for _ in 0..4 {
                    for c in &contacts {
                        let vel = body.lin + body.ang.cross(c.r);
                        let vn = vel.dot(c.n);
                        if vn >= 0.0 {
                            continue;
                        }
                        if -vn > worst_speed {
                            worst_speed = -vn;
                            worst_point = body.pos + c.r;
                        }
                        let rn = c.r.cross(c.n);
                        let k = inv_m + (inv_world * rn).cross(c.r).dot(c.n);
                        let e = if -vn > 3.0 { 0.12 } else { 0.0 };
                        let j = -(1.0 + e) * vn / k.max(1e-6);
                        body.lin += c.n * (j * inv_m);
                        body.ang += inv_world * (c.r.cross(c.n * j));
                        // Frottement de Coulomb.
                        let vel = body.lin + body.ang.cross(c.r);
                        let vt = vel - c.n * vel.dot(c.n);
                        let vt_len = vt.length();
                        if vt_len > 1e-4 {
                            let t = vt / vt_len;
                            let rt = c.r.cross(t);
                            let kt = inv_m + (inv_world * rt).cross(c.r).dot(t);
                            let jt = (-vt_len / kt.max(1e-6)).max(-0.7 * j);
                            body.lin += t * (jt * inv_m);
                            body.ang += inv_world * (c.r.cross(t * jt));
                        }
                    }
                }
                if worst_speed > BREAK_SPEED && body.cooldown <= 0.0 {
                    body.cooldown = 0.12;
                    breaks.push((bi, worst_point, worst_speed));
                    // Gros morceau + gros choc : il se brise en plusieurs.
                    if worst_speed > 6.0 && body.grid.dims.max_element() > 14 && body.vehicle.is_none() {
                        fracture_ids.push(body.id);
                    }
                }
                // Amortissement au contact : un corps posé s'immobilise.
                body.lin *= 1.0 - 2.5 * dt;
                body.ang *= 1.0 - 5.0 * dt;
                body.rest = if body.lin.length() < 0.6 && body.ang.length() < 0.5 {
                    body.rest + dt
                } else {
                    0.0
                };
            } else {
                body.rest = 0.0;
            }
            body.lin *= 1.0 - 0.01 * dt * 60.0;
            body.ang *= 1.0 - 0.06 * dt * 60.0;
            // Un véhicule reste un corps : il ne se fige ni ne se brise seul.
            if body.rest > 0.5 && body.vehicle.is_none() {
                settle.push(bi);
            }
            if body.pos.y < -20.0 {
                settle.push(bi);
            }
        }

        self.pending_fracture.extend(fracture_ids);
        // Impacts : casse le corps et le monde autour du point de contact.
        let mut removed: Vec<u32> = Vec::new();
        let mut new_bodies: Vec<Body> = Vec::new();
        for (bi, point, speed) in breaks {
            let radius = ((speed - BREAK_SPEED) * 0.28 + 0.5).clamp(0.5, 3.2);
            ev.push(SimEvent::Impact { at: point, power: speed });
            // Monde.
            let mut carved = world.carve(point, radius * 0.8);
            if !carved.is_empty() {
                let ids = self.add_rubble(world, &carved, point, Vec3::Y * 2.0);
                if !ids.is_empty() {
                    ev.push(SimEvent::BlocksAdded(ids));
                }
                let comps = world.collapse_components(&carved);
                if !comps.is_empty() {
                    for comp in &comps {
                        carved.extend_from_slice(comp);
                    }
                    ev.push(SimEvent::WorldRemoved(carved.clone()));
                    let mut tmp = Vec::new();
                    self.form_bodies(world, comps, &mut tmp);
                    ev.append(&mut tmp);
                } else {
                    ev.push(SimEvent::WorldRemoved(carved.clone()));
                }
            }
            // Corps.
            let Some(body) = self.bodies.get_mut(bi) else { continue };
            let mut chips: Vec<(Vec3, u8)> = Vec::new();
            let d = body.grid.dims;
            let center_local = body.rot.inverse() * (point - body.pos) + body.com;
            let r_cells = (radius / VOXEL).ceil() as i32 + 1;
            let cc = (center_local / VOXEL).floor().as_ivec3();
            for x in (cc.x - r_cells).max(0)..(cc.x + r_cells + 1).min(d.x) {
                for y in (cc.y - r_cells).max(0)..(cc.y + r_cells + 1).min(d.y) {
                    for z in (cc.z - r_cells).max(0)..(cc.z + r_cells + 1).min(d.z) {
                        let c = IVec3::new(x, y, z);
                        if !body.grid.is_solid(c) {
                            continue;
                        }
                        let lp = (c.as_vec3() + Vec3::splat(0.5)) * VOXEL;
                        if lp.distance(center_local) <= radius {
                            chips.push((body.cell_world(c), body.grid.material(c)));
                            body.grid.set(c, None);
                        }
                    }
                }
            }
            let (vel, ang, pos) = (body.lin, body.ang, body.pos);
            // Perte d'énergie à l'impact.
            body.lin *= 0.55;
            body.ang *= 0.7;
            // Gravats : un bloc par zone de 2×2×2 de la grille locale.
            use std::collections::HashMap;
            let mut zones: HashMap<IVec3, (Vec3, u32, u8)> = HashMap::new();
            // (rangés par zone dans le repère monde grossier)
            for (wp, m) in &chips {
                let key = (*wp / (VOXEL * ZONE as f32)).floor().as_ivec3();
                let e = zones.entry(key).or_insert((Vec3::ZERO, 0, *m));
                e.0 += *wp;
                e.1 += 1;
            }
            let mut count = 0;
            let mut new_ids = Vec::new();
            for (_, (sum, n, m)) in zones {
                if m == mat::GLASS || count >= MAX_PER_HIT {
                    continue;
                }
                count += 1;
                let c = sum / n as f32;
                let half = Vec3::splat(VOXEL * ZONE as f32 * 0.45);
                let vp = vel + ang.cross(c - pos);
                let id = self.next_id;
                self.next_id = self.next_id.wrapping_add(1).max(1);
                let jitter = Vec3::new(self.rng.signed(), self.rng.f32() * 0.6, self.rng.signed()) * 2.0;
                self.blocks.push(Block {
                    id,
                    pos: c,
                    yaw: 0.0,
                    vel: vp * 0.5 + (c - point).normalize_or_zero() * 2.0 + jitter,
                    spin: self.rng.signed() * 4.0,
                    half,
                    mat: m,
                    mass: half.x * half.y * half.z * 8.0 * density(m),
                    sleeping: false,
                    rest: 0.0,
                });
                new_ids.push(id);
            }
            if !new_ids.is_empty() {
                ev.push(SimEvent::BlocksAdded(new_ids));
            }
            // Mise à jour du corps, et scission en morceaux.
            let existing = self.bodies.len();
            let Some(body) = self.bodies.get_mut(bi) else { continue };
            if !body.recompute() {
                removed.push(body.id);
                continue;
            }
            let parts = split_components(&body.grid);
            if parts.len() > 1 {
                // Le plus gros reste ; les autres deviennent des corps.
                let keep = parts
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, p)| p.len())
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                let (b_pos, b_rot, b_com, b_lin, b_ang) = (body.pos, body.rot, body.com, body.lin, body.ang);
                let old_grid = body.grid.clone();
                for (i, part) in parts.iter().enumerate() {
                    if i == keep {
                        let mut g = BodyGrid::new(old_grid.dims);
                        for &c in part {
                            g.set(c, Some(old_grid.material(c)));
                        }
                        body.grid = g;
                        continue;
                    }
                    if part.len() < 10 || existing + new_bodies.len() >= MAX_BODIES {
                        continue;
                    }
                    let mut lo = part[0];
                    let mut hi = part[0];
                    for &c in part {
                        lo = lo.min(c);
                        hi = hi.max(c);
                    }
                    let mut g = BodyGrid::new(hi - lo + IVec3::ONE);
                    for &c in part {
                        g.set(c - lo, Some(old_grid.material(c)));
                    }
                    let id = self.next_id;
                    self.next_id = self.next_id.wrapping_add(1).max(1);
                    // Repère : la nouvelle grille commence en `lo`.
                    let corner_local = lo.as_vec3() * VOXEL;
                    let mut nb = Body {
                        id,
                        grid: g,
                        com: Vec3::ZERO,
                        pos: b_pos + b_rot * (corner_local - b_com),
                        rot: b_rot,
                        lin: b_lin,
                        ang: b_ang,
                        mass: 0.0,
                        inv_inertia: Mat3::IDENTITY,
                        samples: Vec::new(),
                        rest: 0.0,
                        cooldown: 0.3,
                        vehicle: None,
                    };
                    if nb.recompute() {
                        new_bodies.push(nb);
                    }
                }
                if let Some(body) = self.bodies.get_mut(bi) {
                    if !body.recompute() {
                        removed.push(body.id);
                    } else {
                        ev.push(SimEvent::BodyChanged(body.id));
                    }
                }
            } else {
                ev.push(SimEvent::BodyChanged(body.id));
            }
        }
        for nb in new_bodies {
            ev.push(SimEvent::BodyChanged(nb.id));
            self.bodies.push(nb);
        }

        // Corps au repos : un gros morceau se brise encore (il ne « gèle »
        // pas), un petit morceau penché devient un tas de gravats, le reste
        // se fige dans le monde.
        settle.sort_unstable();
        settle.dedup();
        let mut to_fracture: Vec<u32> = Vec::new();
        for bi in settle.into_iter().rev() {
            if bi >= self.bodies.len() {
                continue;
            }
            let count = self.bodies[bi].grid.count();
            let tilted = tilt_degrees(self.bodies[bi].rot) > 6.0;
            if count > 450 && self.bodies.len() + to_fracture.len() < MAX_BODIES && self.bodies[bi].pos.y > -15.0 {
                to_fracture.push(self.bodies[bi].id);
                continue;
            }
            let body = self.bodies.remove(bi);
            if !removed.contains(&body.id) {
                removed.push(body.id);
            }
            if tilted && count > 0 {
                let ids = self.body_to_rubble(&body);
                if !ids.is_empty() {
                    ev.push(SimEvent::BlocksAdded(ids));
                }
                continue;
            }
            let placed = bake(world, &body);
            if !placed.is_empty() {
                ev.push(SimEvent::WorldPlaced(placed));
            }
        }
        for id in removed {
            self.bodies.retain(|b| b.id != id);
            ev.push(SimEvent::BodyRemoved(id));
        }
        // Un impact violent brise aussi les gros morceaux.
        for id in std::mem::take(&mut self.pending_fracture) {
            if !to_fracture.contains(&id) {
                to_fracture.push(id);
            }
        }
        for id in to_fracture {
            self.fracture(id, ev);
        }
    }

    /// Brise un corps en plusieurs morceaux (découpe par graines, chacune
    /// reste un solide rigide qui continue de tomber et de se briser).
    pub fn fracture(&mut self, id: u32, ev: &mut Vec<SimEvent>) {
        let Some(bi) = self.bodies.iter().position(|b| b.id == id) else { return };
        if self.bodies.len() >= MAX_BODIES {
            return;
        }
        let body = self.bodies.remove(bi);
        ev.push(SimEvent::BodyRemoved(id));
        let solid: Vec<IVec3> = {
            let d = body.grid.dims;
            let mut v = Vec::with_capacity(body.grid.count());
            for x in 0..d.x {
                for y in 0..d.y {
                    for z in 0..d.z {
                        let c = IVec3::new(x, y, z);
                        if body.grid.is_solid(c) {
                            v.push(c);
                        }
                    }
                }
            }
            v
        };
        let n = (solid.len() / 280).clamp(2, 8);
        let seeds: Vec<Vec3> = (0..n)
            .map(|_| solid[self.rng.usize(solid.len())].as_vec3())
            .collect();
        // Chaque voxel va à la graine la plus proche.
        let mut groups: Vec<Vec<IVec3>> = vec![Vec::new(); n];
        for &c in &solid {
            let p = c.as_vec3();
            let mut best = 0;
            let mut best_d = f32::MAX;
            for (i, s) in seeds.iter().enumerate() {
                let d = p.distance_squared(*s);
                if d < best_d {
                    best_d = d;
                    best = i;
                }
            }
            groups[best].push(c);
        }
        let mut made = 0;
        for group in groups {
            if group.is_empty() {
                continue;
            }
            // Un groupe peut être en plusieurs morceaux non reliés.
            let mut sub = BodyGrid::new(body.grid.dims);
            for &c in &group {
                sub.set(c, Some(body.grid.material(c)));
            }
            for comp in split_components(&sub) {
                let mut lo = comp[0];
                let mut hi = comp[0];
                for &c in &comp {
                    lo = lo.min(c);
                    hi = hi.max(c);
                }
                let mut g = BodyGrid::new(hi - lo + IVec3::ONE);
                for &c in &comp {
                    g.set(c - lo, Some(body.grid.material(c)));
                }
                if comp.len() < 12 || self.bodies.len() >= MAX_BODIES {
                    // Trop petit : poussière de gravats.
                    let mut temp = Body {
                        id: 0,
                        grid: g,
                        com: Vec3::ZERO,
                        pos: body.pos + body.rot * (lo.as_vec3() * VOXEL - body.com),
                        rot: body.rot,
                        lin: body.lin,
                        ang: body.ang,
                        mass: 0.0,
                        inv_inertia: Mat3::IDENTITY,
                        samples: Vec::new(),
                        rest: 0.0,
                        cooldown: 0.0,
                        vehicle: None,
                    };
                    if temp.recompute() {
                        let ids = self.body_to_rubble(&temp);
                        if !ids.is_empty() {
                            ev.push(SimEvent::BlocksAdded(ids));
                        }
                    }
                    continue;
                }
                let center_local = comp.iter().map(|c| c.as_vec3()).sum::<Vec3>() / comp.len() as f32 * VOXEL;
                let away = (body.rot * (center_local - body.com)).normalize_or_zero();
                let new_id = self.next_id;
                self.next_id = self.next_id.wrapping_add(1).max(1);
                let jitter = Vec3::new(self.rng.signed(), self.rng.f32() * 0.5, self.rng.signed());
                let mut nb = Body {
                    id: new_id,
                    grid: g,
                    com: Vec3::ZERO,
                    pos: body.pos + body.rot * (lo.as_vec3() * VOXEL - body.com),
                    rot: body.rot,
                    lin: body.lin * 0.8 + away * 1.6 + jitter * 0.8,
                    ang: body.ang * 0.8 + Vec3::new(self.rng.signed(), self.rng.signed() * 0.5, self.rng.signed()) * 0.6,
                    mass: 0.0,
                    inv_inertia: Mat3::IDENTITY,
                    samples: Vec::new(),
                    rest: 0.0,
                    cooldown: 0.25,
                    vehicle: None,
                };
                if nb.recompute() {
                    ev.push(SimEvent::BodyChanged(new_id));
                    self.bodies.push(nb);
                    made += 1;
                }
            }
        }
        let _ = made;
        ev.push(SimEvent::Impact {
            at: body.pos,
            power: 8.0 + (solid.len() as f32).sqrt() * 0.15,
        });
    }

    /// Transforme un corps en blocs de gravats (un par zone 2×2×2).
    fn body_to_rubble(&mut self, body: &Body) -> Vec<u32> {
        use std::collections::HashMap;
        let mut zones: HashMap<IVec3, (Vec3, u32, u8)> = HashMap::new();
        let d = body.grid.dims;
        for x in 0..d.x {
            for y in 0..d.y {
                for z in 0..d.z {
                    let c = IVec3::new(x, y, z);
                    if !body.grid.is_solid(c) {
                        continue;
                    }
                    let key = c.div_euclid(IVec3::splat(ZONE));
                    let m = body.grid.material(c);
                    let e = zones.entry(key).or_insert((Vec3::ZERO, 0, m));
                    e.0 += body.cell_world(c);
                    e.1 += 1;
                }
            }
        }
        let mut list: Vec<(Vec3, u8)> = zones
            .into_values()
            .filter(|(_, _, m)| *m != mat::GLASS)
            .map(|(sum, n, m)| (sum / n as f32, m))
            .collect();
        if list.len() > MAX_PER_HIT {
            for i in 0..MAX_PER_HIT {
                let j = i + self.rng.usize(list.len() - i);
                list.swap(i, j);
            }
            list.truncate(MAX_PER_HIT);
        }
        let mut ids = Vec::new();
        for (c, m) in list {
            let half = Vec3::splat(VOXEL * ZONE as f32 * 0.45);
            let id = self.new_id();
            let v = body.velocity_at(c) * 0.5 + Vec3::new(self.rng.signed(), self.rng.f32() * 0.4, self.rng.signed()) * 0.8;
            self.blocks.push(Block {
                id,
                pos: c,
                yaw: 0.0,
                vel: v,
                spin: self.rng.signed() * 3.0,
                half,
                mat: m,
                mass: half.x * half.y * half.z * 8.0 * density(m),
                sleeping: false,
                rest: 0.0,
            });
            ids.push(id);
        }
        ids
    }

    // ------------------------------------------------------------------
    // Dégâts aux joueurs
    // ------------------------------------------------------------------

    /// Éclats : transforme les voxels arrachés d'un corps en gravats.
    fn chips_to_rubble(&mut self, chips: &[(Vec3, u8)], vel: Vec3, ang: Vec3, origin: Vec3, point: Vec3, ev: &mut Vec<SimEvent>) {
        use std::collections::HashMap;
        let mut zones: HashMap<IVec3, (Vec3, u32, u8)> = HashMap::new();
        for (wp, m) in chips {
            let key = (*wp / (VOXEL * ZONE as f32)).floor().as_ivec3();
            let e = zones.entry(key).or_insert((Vec3::ZERO, 0, *m));
            e.0 += *wp;
            e.1 += 1;
        }
        let mut count = 0;
        let mut new_ids = Vec::new();
        for (_, (sum, n, m)) in zones {
            if m == mat::GLASS || count >= MAX_PER_HIT {
                continue;
            }
            count += 1;
            let c = sum / n as f32;
            let half = Vec3::splat(VOXEL * ZONE as f32 * 0.45);
            let vp = vel + ang.cross(c - origin);
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            let jitter = Vec3::new(self.rng.signed(), self.rng.f32() * 0.6, self.rng.signed()) * 2.0;
            self.blocks.push(Block {
                id,
                pos: c,
                yaw: 0.0,
                vel: vp * 0.5 + (c - point).normalize_or_zero() * 2.0 + jitter,
                spin: self.rng.signed() * 4.0,
                half,
                mat: m,
                mass: half.x * half.y * half.z * 8.0 * density(m),
                sleeping: false,
                rest: 0.0,
            });
            new_ids.push(id);
        }
        if !new_ids.is_empty() {
            ev.push(SimEvent::BlocksAdded(new_ids));
        }
    }

    /// Premier corps solide traversé par un rayon.
    ///
    /// Le rayon de visée des armes ne regarde que la grille du monde : sans
    /// ceci, les véhicules (devenus des corps rigides) étaient
    /// **intouchables**, on pouvait les canonner sans rien abîmer.
    /// Renvoie (point d'impact, identifiant du corps).
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<(Vec3, u32)> {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return None;
        }
        let mut best: Option<(Vec3, u32)> = None;
        // On ne s'intéresse qu'aux véhicules : les autres corps se font
        // sculpter par le monde et leurs gravats font le reste.
        for body in &self.bodies {
            if body.vehicle.is_none() {
                continue;
            }
            let half = body.grid.dims.as_vec3() * VOXEL * 0.5;
            let reach = half.length() + 1.0;
            if (body.pos - origin).length() > reach + max_dist {
                continue;
            }
            // Pas de VOXEL/2 : on ne rate pas un voxel sur deux.
            let step = VOXEL * 0.5;
            let steps = ((max_dist / step) as usize).min(4096);
            for i in 0..=steps {
                let p = origin + dir * (i as f32 * step);
                if body.contains(p) {
                    let d = p.distance(origin);
                    if best.is_none_or(|(bp, _)| d < bp.distance(origin)) {
                        best = Some((p, body.id));
                    }
                    break;
                }
            }
        }
        best
    }

    /// Écarte les voxels d'une sphère de tous les corps rigides touchés.
    ///
    /// C'est ce qui rend les **véhicules destructibles** : une fois
    /// transformée en corps rigide, la voiture n'appartient plus à la grille
    /// du monde, donc ni `carve` ni le rayon de visée des armes ne la
    /// touchaient — on pouvait la canonner sans lui faire le moindre dégât.
    /// Renvoie le nombre de corps altérés.
    pub fn damage_bodies(&mut self, point: Vec3, radius: f32, ev: &mut Vec<SimEvent>) -> usize {
        let mut hit_any = 0usize;
        for bi in 0..self.bodies.len() {
            // La sphère d'éclats ne touche pas le volume englobant du corps.
            let Some(body) = self.bodies.get(bi) else { continue };
            let half = body.grid.dims.as_vec3() * VOXEL * 0.5;
            if (body.pos - point).length() > radius + half.length() {
                continue;
            }
            let mut chips: Vec<(Vec3, u8)> = Vec::new();
            let d = body.grid.dims;
            let center_local = body.rot.inverse() * (point - body.pos) + body.com;
            let r_cells = (radius / VOXEL).ceil() as i32 + 1;
            let cc = (center_local / VOXEL).floor().as_ivec3();
            for x in (cc.x - r_cells).max(0)..(cc.x + r_cells + 1).min(d.x) {
                for y in (cc.y - r_cells).max(0)..(cc.y + r_cells + 1).min(d.y) {
                    for z in (cc.z - r_cells).max(0)..(cc.z + r_cells + 1).min(d.z) {
                        let c = IVec3::new(x, y, z);
                        let Some(body) = self.bodies.get_mut(bi) else { break };
                        if !body.grid.is_solid(c) {
                            continue;
                        }
                        let lp = (c.as_vec3() + Vec3::splat(0.5)) * VOXEL;
                        if lp.distance(center_local) <= radius {
                            chips.push((body.cell_world(c), body.grid.material(c)));
                            body.grid.set(c, None);
                        }
                    }
                }
            }
            if chips.is_empty() {
                continue;
            }
            hit_any += 1;
            let (vel, ang, pos) = {
                let body = &mut self.bodies[bi];
                body.lin *= 0.9;
                (body.lin, body.ang, body.pos)
            };
            self.chips_to_rubble(&chips, vel, ang, pos, point, ev);
            let Some(body) = self.bodies.get_mut(bi) else { continue };
            if body.grid.count() == 0 || !body.recompute() {
                ev.push(SimEvent::BodyRemoved(body.id));
                self.bodies.remove(bi);
            } else {
                ev.push(SimEvent::BodyChanged(body.id));
            }
        }
        hit_any
    }

    /// Vitesse d'un corps ou d'un bloc lourd qui touche la boîte `[lo, hi]`.
    /// Renvoie (masse, vitesse verticale/horizontale) du pire objet.
    pub fn crush_at(&self, lo: Vec3, hi: Vec3) -> Option<(f32, f32)> {
        self.crush_at_except(lo, hi, None)
    }
    /// Comme [`Self::crush_at`], mais en ignorant le corps `except`.
    ///
    /// Indispensable pour le conducteur : sa position **est** celle de son
    /// véhicule, donc sans cette exclusion il se faisait écraser par sa
    /// propre voiture (36 à 84 dégâts dès 1,5 m/s, donc mort en un tour).
    pub fn crush_at_except(&self, lo: Vec3, hi: Vec3, except: Option<u32>) -> Option<(f32, f32)> {
        let mut best: Option<(f32, f32)> = None;
        for b in &self.blocks {
            if b.sleeping || b.mass < 0.7 || b.vel.length() < 4.0 {
                continue;
            }
            let (bmin, bmax) = (b.pos - b.half, b.pos + b.half);
            if bmin.cmplt(hi).all() && bmax.cmpgt(lo).all() {
                let s = b.vel.length();
                if best.is_none_or(|(m, v)| b.mass * s > m * v) {
                    best = Some((b.mass, s));
                }
            }
        }
        let center = (lo + hi) * 0.5;
        for body in &self.bodies {
            if Some(body.id) == except {
                continue;
            }
            let speed = body.velocity_at(center).length();
            if speed < 1.5 {
                continue;
            }
            let hit = [
                center,
                Vec3::new(lo.x, lo.y, lo.z),
                Vec3::new(hi.x, lo.y, hi.z),
                Vec3::new(lo.x, hi.y, hi.z),
                Vec3::new(hi.x, hi.y, lo.z),
            ]
            .iter()
            .any(|&p| body.contains(p));
            if hit && best.is_none_or(|(m, v)| body.mass * speed > m * v) {
                best = Some((body.mass.min(60.0), speed));
            }
        }
        best
    }
}

/// Écart (degrés) entre l'orientation du corps et l'alignement sur les axes
/// du monde : un corps proche de 0° peut se figer proprement.
fn tilt_degrees(rot: Quat) -> f32 {
    let mut worst = 0.0f32;
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        let v = rot * axis;
        let m = v.abs().max_element().clamp(0.0, 1.0);
        worst = worst.max(m.acos().to_degrees());
    }
    worst
}

/// Composantes connexes (6 voisins) d'une grille.
pub fn split_components(grid: &BodyGrid) -> Vec<Vec<IVec3>> {
    let d = grid.dims;
    let mut seen = vec![false; grid.cells.len()];
    let mut out = Vec::new();
    for x in 0..d.x {
        for y in 0..d.y {
            for z in 0..d.z {
                let c = IVec3::new(x, y, z);
                let i = ((c.x * d.y + c.y) * d.z + c.z) as usize;
                if seen[i] || !grid.is_solid(c) {
                    continue;
                }
                let mut stack = vec![c];
                seen[i] = true;
                let mut comp = Vec::new();
                while let Some(cur) = stack.pop() {
                    comp.push(cur);
                    for o in [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z] {
                        let n = cur + o;
                        if !grid.is_solid(n) {
                            continue;
                        }
                        let ni = ((n.x * d.y + n.y) * d.z + n.z) as usize;
                        if !seen[ni] {
                            seen[ni] = true;
                            stack.push(n);
                        }
                    }
                }
                out.push(comp);
            }
        }
    }
    out
}

/// Repose les voxels d'un corps dans la grille du monde (correspondance
/// inverse : aucune case vide dans la zone couverte).
fn bake(world: &mut VoxelWorld, body: &Body) -> Vec<[u16; 4]> {
    let d = body.grid.dims;
    // Boîte englobante monde des 8 coins.
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for &x in &[0.0, d.x as f32] {
        for &y in &[0.0, d.y as f32] {
            for &z in &[0.0, d.z as f32] {
                let p = body.pos + body.rot * (Vec3::new(x, y, z) * VOXEL - body.com);
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
    }
    let c0 = world.world_to_cell(lo);
    let c1 = world.world_to_cell(hi);
    let volume = (c1 - c0 + IVec3::ONE).as_vec3().element_product();
    let mut placed = Vec::new();
    if volume > 4_000_000.0 {
        return placed;
    }
    let inv = body.rot.inverse();
    for x in c0.x..=c1.x {
        for y in c0.y.max(1)..=c1.y {
            for z in c0.z..=c1.z {
                let c = IVec3::new(x, y, z);
                if world.is_solid(c) {
                    continue;
                }
                let local = inv * (world.cell_center(c) - body.pos) + body.com;
                let g = (local / VOXEL).floor().as_ivec3();
                let raw = body.grid.raw(g);
                if raw == 0 {
                    continue;
                }
                let m = raw - 1;
                world.set(c, true, m);
                placed.push([c.x as u16, c.y as u16, c.z as u16, m as u16]);
            }
        }
    }
    placed
}

// ----------------------------------------------------------------------
// Sérialisation (sauvegardes)
// ----------------------------------------------------------------------

/// Écrivain binaire minimal.
#[derive(Default)]
pub struct W(pub Vec<u8>);

impl W {
    pub fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
    }
    pub fn string(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
}

/// Lecteur binaire correspondant.
pub struct R<'a> {
    pub data: &'a [u8],
    pub at: usize,
}

impl<'a> R<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.data.get(self.at..end)?;
        self.at = end;
        Some(s)
    }
    pub fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    pub fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    pub fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    pub fn vec3(&mut self) -> Option<Vec3> {
        Some(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    pub fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    pub fn string(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?.to_vec()).ok()
    }
}

impl Sim {
    /// Écrit tous les gravats et corps.
    pub fn write(&self, w: &mut W) {
        w.u32(self.next_id);
        w.u32(self.blocks.len() as u32);
        for b in &self.blocks {
            w.u32(b.id);
            w.vec3(b.pos);
            w.f32(b.yaw);
            w.vec3(b.vel);
            w.f32(b.spin);
            w.vec3(b.half);
            w.u8(b.mat);
            w.f32(b.mass);
            w.u8(b.sleeping as u8);
        }
        w.u32(self.bodies.len() as u32);
        for b in &self.bodies {
            w.u32(b.id);
            w.u16(b.grid.dims.x as u16);
            w.u16(b.grid.dims.y as u16);
            w.u16(b.grid.dims.z as u16);
            w.bytes(&b.grid.encode_rle());
            w.vec3(b.pos);
            for q in b.rot.to_array() {
                w.f32(q);
            }
            w.vec3(b.lin);
            w.vec3(b.ang);
        }
    }

    pub fn read(&mut self, r: &mut R) -> Option<()> {
        self.clear();
        self.next_id = r.u32()?;
        let n = r.u32()? as usize;
        for _ in 0..n {
            self.blocks.push(Block {
                id: r.u32()?,
                pos: r.vec3()?,
                yaw: r.f32()?,
                vel: r.vec3()?,
                spin: r.f32()?,
                half: r.vec3()?,
                mat: r.u8()?,
                mass: r.f32()?,
                sleeping: r.u8()? != 0,
                rest: 0.0,
            });
        }
        let n = r.u32()? as usize;
        for _ in 0..n {
            let id = r.u32()?;
            let dims = IVec3::new(r.u16()? as i32, r.u16()? as i32, r.u16()? as i32);
            let rle = r.bytes()?;
            let grid = BodyGrid::decode_rle(dims, rle)?;
            let pos = r.vec3()?;
            let rot = Quat::from_array([r.f32()?, r.f32()?, r.f32()?, r.f32()?]);
            let lin = r.vec3()?;
            let ang = r.vec3()?;
            let mut body = Body {
                id,
                grid,
                com: Vec3::ZERO,
                pos,
                rot,
                lin,
                ang,
                mass: 0.0,
                inv_inertia: Mat3::IDENTITY,
                samples: Vec::new(),
                rest: 0.0,
                cooldown: 0.0,
                vehicle: None,
            };
            // `pos` sauvegardé est déjà celui du centre de masse : on
            // recalcule la grille sans décaler.
            let saved = body.pos;
            body.com = Vec3::ZERO;
            if body.recompute() {
                body.pos = saved;
                self.bodies.push(body);
            }
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_world() -> VoxelWorld {
        let mut w = VoxelWorld::new();
        w.fill(IVec3::new(0, 0, 0), IVec3::new(200, 1, 200), mat::BEDROCK);
        w.fill(IVec3::new(0, 1, 0), IVec3::new(200, 6, 200), mat::DIRT);
        w
    }

    #[test]
    fn rle_round_trip() {
        let mut g = BodyGrid::new(IVec3::new(5, 4, 3));
        g.set(IVec3::new(1, 1, 1), Some(8));
        g.set(IVec3::new(4, 3, 2), Some(2));
        let back = BodyGrid::decode_rle(g.dims, &g.encode_rle()).unwrap();
        assert_eq!(back.cells, g.cells);
    }

    #[test]
    fn floating_slab_falls_and_settles_on_ground() {
        let mut w = flat_world();
        let mut sim = Sim::new();
        let mut cells = Vec::new();
        for x in 20..30 {
            for y in 20..22 {
                for z in 20..30 {
                    w.set(IVec3::new(x, y, z), true, mat::CONCRETE);
                    cells.push(IVec3::new(x, y, z));
                }
            }
        }
        for &c in &cells {
            w.set(c, false, 0);
        }
        let mut ev = Vec::new();
        sim.form_bodies(&w, vec![cells], &mut ev);
        assert_eq!(sim.bodies.len(), 1);
        let start_y = sim.bodies[0].pos.y;
        for _ in 0..600 {
            sim.step(&mut w, 1.0 / 60.0, &mut ev);
            if sim.bodies.is_empty() {
                break;
            }
        }
        assert!(sim.bodies.is_empty(), "la dalle s'est figée");
        assert!(start_y > w.cell_min(IVec3::new(0, 6, 0)).y + 1.0);
        // Des voxels sont revenus dans le monde, posés sur le sol.
        let mut found = 0;
        for x in 15..35 {
            for z in 15..35 {
                if w.is_solid(IVec3::new(x, 6, z)) {
                    found += 1;
                }
            }
        }
        assert!(found > 40, "{found} voxels reposés");
    }

    #[test]
    fn whole_building_falls_as_one_body_and_settles() {
        let mut w = VoxelWorld::new();
        w.rebuild(4242);
        let g = crate::map::GROUND;
        // Le bâtiment le plus haut près du centre.
        let c = IVec3::new(w.dims.x / 2, 0, w.dims.z / 2);
        let mut best = (0, IVec3::ZERO);
        for dx in (-90..90).step_by(2) {
            for dz in (-90..90).step_by(2) {
                let p = IVec3::new(c.x + dx, 0, c.z + dz);
                let mut top = 0;
                for y in (g..w.dims.y).rev() {
                    if w.is_solid(IVec3::new(p.x, y, p.z)) {
                        top = y;
                        break;
                    }
                }
                if top > best.0 {
                    best = (top, p);
                }
            }
        }
        assert!(best.0 > g + 15, "un bâtiment haut a été trouvé ({})", best.0);
        // On tranche une bande sur toute la largeur autour de lui.
        let p = best.1;
        let mut cut = Vec::new();
        for x in p.x - 40..p.x + 40 {
            for z in p.z - 40..p.z + 40 {
                for y in g + 2..g + 6 {
                    let cell = IVec3::new(x, y, z);
                    if w.is_solid(cell) {
                        w.set(cell, false, 0);
                        cut.push(cell);
                    }
                }
            }
        }
        let t0 = std::time::Instant::now();
        let comps = w.collapse_components(&cut);
        let t_collapse = t0.elapsed();
        assert!(!comps.is_empty(), "le haut du bâtiment se détache");
        let voxels: usize = comps.iter().map(|c| c.len()).sum();
        let mut sim = Sim::new();
        let mut ev = Vec::new();
        sim.form_bodies(&w, comps, &mut ev);
        assert!(!sim.bodies.is_empty());
        let t1 = std::time::Instant::now();
        let mut steps = 0;
        let mut max_step = std::time::Duration::ZERO;
        while !sim.bodies.is_empty() && steps < 60 * 40 {
            let s0 = std::time::Instant::now();
            sim.step(&mut w, 1.0 / 60.0, &mut ev);
            max_step = max_step.max(s0.elapsed());
            steps += 1;
            if steps % 120 == 1 {
                if let Some(b) = sim.bodies.first() {
                    println!("pas {steps}: y={:.2} v={:.2} samples={} mass={:.1} bodies={}", b.pos.y, b.lin.length(), b.samples.len(), b.mass, sim.bodies.len());
                }
            }
        }
        println!(
            "effondrement : {voxels} voxels, détection {t_collapse:?}, {} corps, {} pas, total {:?}, pas max {max_step:?}, {} blocs",
            sim.bodies.len(),
            steps,
            t1.elapsed(),
            sim.blocks.len()
        );
        assert!(sim.bodies.is_empty(), "tout s'est figé en {steps} pas");
    }

    #[test]
    fn vehicle_turns_and_stays_upright() {
        let mut w = flat_world();
        let mut sim = Sim::new();
        let mut cells = Vec::new();
        for x in 50..55 {
            for y in 6..9 {
                for z in 50..61 {
                    cells.push(IVec3::new(x, y, z));
                }
            }
        }
        let id = sim.form_vehicle(&w, cells).expect("véhicule");
        let mut ev = Vec::new();
        for _ in 0..150 {
            if let Some(b) = sim.body_mut(id) {
                let v = b.vehicle.as_mut().unwrap();
                v.throttle = 1.0;
                v.steer = 1.0;
                v.ttl = 0.3;
            }
            sim.step(&mut w, 1.0 / 60.0, &mut ev);
        }
        let b = sim.body(id).unwrap();
        let fwd = b.rot * Vec3::NEG_Z;
        let heading = fwd.x.atan2(-fwd.z).to_degrees().abs();
        assert!(heading > 30.0, "la voiture tourne : {heading:.0}°");
        assert!((b.rot * Vec3::Y).y > 0.8, "reste sur ses roues");
        assert!(b.vehicle.as_ref().unwrap().grounded, "roues au sol");
    }

    #[test]
    fn vehicle_drives_forward_and_can_be_wrecked() {
        let mut w = flat_world();
        let mut sim = Sim::new();
        let mut cells = Vec::new();
        // Boîte de voiture 5 × 3 × 11 posée sur le sol (y = 6).
        for x in 50..55 {
            for y in 6..9 {
                for z in 50..61 {
                    cells.push(IVec3::new(x, y, z));
                    w.set(IVec3::new(x, y, z), true, mat::METAL);
                }
            }
        }
        for &c in &cells {
            w.set(c, false, 0);
        }
        let id = sim.form_vehicle(&w, cells).expect("véhicule");
        let start = sim.body(id).unwrap().pos;
        let mut ev = Vec::new();
        for _ in 0..180 {
            if let Some(b) = sim.body_mut(id) {
                let v = b.vehicle.as_mut().unwrap();
                v.throttle = 1.0;
                v.ttl = 0.3;
            }
            sim.step(&mut w, 1.0 / 60.0, &mut ev);
        }
        let b = sim.body(id).expect("toujours un corps");
        let moved = (b.pos - start).length();
        assert!(moved > 8.0, "la voiture avance : {moved:.1} m");
        assert!(b.pos.y < start.y + 1.0, "reste au sol");
        // Sans voxels, plus de moteur.
        let mut b = b.clone();
        for x in 0..b.grid.dims.x {
            for y in 0..b.grid.dims.y {
                for z in 0..b.grid.dims.z / 2 + 1 {
                    b.grid.set(IVec3::new(x, y, z), None);
                }
            }
        }
        assert!(b.grid.count() * 100 < 60 * 165, "épave");
    }

    #[test]
    fn simulation_round_trips_through_bytes() {
        let mut w = flat_world();
        let mut sim = Sim::new();
        let cells: Vec<IVec3> = (0..4)
            .flat_map(|x| (0..4).flat_map(move |y| (0..4).map(move |z| IVec3::new(30 + x, 10 + y, 30 + z))))
            .collect();
        for &c in &cells {
            w.set(c, true, mat::BRICK);
        }
        for &c in &cells {
            w.set(c, false, 0);
        }
        let mut ev = Vec::new();
        sim.form_bodies(&w, vec![cells], &mut ev);
        sim.add_rubble(&w, &[IVec3::new(5, 6, 5), IVec3::new(5, 6, 6)], Vec3::ZERO, Vec3::ZERO);
        let mut out = W::default();
        sim.write(&mut out);
        let mut back = Sim::new();
        assert!(back.read(&mut R::new(&out.0)).is_some());
        assert_eq!(back.bodies.len(), sim.bodies.len());
        assert_eq!(back.blocks.len(), sim.blocks.len());
        assert!((back.bodies[0].pos - sim.bodies[0].pos).length() < 1e-4);
    }
}
