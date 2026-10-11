//! Le monde destructible : une grille de voxels compacte.
//!
//! Stockage : un **bitset** pour l'occupation (1 bit par cellule) et un
//! octet pour le matériau. Une carte de 51 × 22 × 51 m occupe ~700 ko, et le
//! rendu ne fait qu'**un seul maillage fusionné** (voir `vmesh`), donc plus
//! aucune entité par voxel : c'est ce qui permet d'avoir des bâtiments et un
//! sol entièrement destructibles sans exploser la RAM ou le GPU.

use bevy::prelude::*;

/// Taille d'un voxel, en mètres.
pub const VOXEL: f32 = 0.4;

/// Dimensions de la grille (X, Y, Z) en cellules.
pub const GRID: [i32; 3] = [128, 56, 128];

/// Résultat d'un lancer de rayon.
pub struct RayHit {
    pub point: Vec3,
    /// Normale de la face touchée (nulle si le rayon démarre dans un voxel).
    pub normal: Vec3,
}

#[derive(Resource)]
pub struct VoxelWorld {
    pub dims: IVec3,
    /// Coin « bas » monde de la cellule (0, 0, 0).
    pub origin: Vec3,
    /// Bitset d'occupation.
    solid: Vec<u64>,
    /// Matériau par cellule.
    mat: Vec<u8>,
    pub alive: u32,
    pub total: u32,
    /// Le rendu doit être reconstruit.
    pub dirty: bool,
    /// Zone touchée depuis le dernier rafraîchissement du rendu.
    dirty_box: Option<(IVec3, IVec3)>,
}

impl Default for VoxelWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl VoxelWorld {
    pub fn new() -> Self {
        let dims = IVec3::from(GRID);
        let count = (dims.x * dims.y * dims.z) as usize;
        let size = dims.as_vec3() * VOXEL;
        Self {
            dims,
            origin: Vec3::new(-size.x * 0.5, 0.0, -size.z * 0.5),
            solid: vec![0; count.div_ceil(64)],
            mat: vec![0; count],
            alive: 0,
            total: 0,
            dirty: true,
            dirty_box: None,
        }
    }

    /// Marque une zone à reconstruire et retourne celle en attente.
    pub fn take_dirty_box(&mut self) -> Option<(IVec3, IVec3)> {
        self.dirty = false;
        self.dirty_box.take()
    }

    fn touch(&mut self, c: IVec3) {
        self.dirty = true;
        self.dirty_box = Some(match self.dirty_box {
            Some((a, b)) => (a.min(c), b.max(c)),
            None => (c, c),
        });
    }

    #[inline]
    pub fn index(&self, c: IVec3) -> Option<usize> {
        if c.x < 0 || c.y < 0 || c.z < 0 || c.x >= self.dims.x || c.y >= self.dims.y || c.z >= self.dims.z
        {
            return None;
        }
        Some(((c.x * self.dims.y + c.y) * self.dims.z + c.z) as usize)
    }

    #[inline]
    pub fn is_solid(&self, c: IVec3) -> bool {
        match self.index(c) {
            Some(i) => self.solid[i >> 6] & (1u64 << (i & 63)) != 0,
            None => false,
        }
    }

    /// Matériau de la cellule (0 si vide).
    #[inline]
    pub fn material(&self, c: IVec3) -> u8 {
        self.index(c).map(|i| self.mat[i]).unwrap_or(0)
    }

    /// Occupe (ou libère) une cellule.
    pub fn set(&mut self, c: IVec3, solid: bool, material: u8) {
        let Some(i) = self.index(c) else { return };
        let bit = 1u64 << (i & 63);
        let was = self.solid[i >> 6] & bit != 0;
        if solid && !was {
            self.solid[i >> 6] |= bit;
            self.alive += 1;
        } else if !solid && was {
            self.solid[i >> 6] &= !bit;
            self.alive = self.alive.saturating_sub(1);
        }
        if solid {
            self.mat[i] = material;
        }
        self.touch(c);
    }

    /// Vide toute la grille.
    pub fn clear(&mut self) {
        self.solid.iter_mut().for_each(|w| *w = 0);
        self.alive = 0;
        self.total = 0;
        self.touch(IVec3::ZERO);
    }

    #[inline]
    pub fn cell_min(&self, c: IVec3) -> Vec3 {
        self.origin + c.as_vec3() * VOXEL
    }

    /// Coin haut (exclusif) d'une cellule.
    #[inline]
    pub fn cell_max(&self, c: IVec3) -> Vec3 {
        self.cell_min(c) + Vec3::splat(VOXEL)
    }

    #[inline]
    pub fn cell_center(&self, c: IVec3) -> Vec3 {
        self.cell_min(c) + Vec3::splat(VOXEL * 0.5)
    }

    #[inline]
    pub fn world_to_cell(&self, p: Vec3) -> IVec3 {
        ((p - self.origin) / VOXEL).floor().as_ivec3()
    }

    /// Appelle `f` sur toutes les cellules dont la boîte chevauche
    /// `[min, max]`.
    pub fn visit_cells<F: FnMut(IVec3)>(&self, min: Vec3, max: Vec3, mut f: F) {
        const EPS: f32 = 1e-4;
        let c0 = self.world_to_cell(min + Vec3::splat(EPS));
        let c1 = self.world_to_cell(max - Vec3::splat(EPS));
        for z in c0.z..=c1.z {
            for y in c0.y..=c1.y {
                for x in c0.x..=c1.x {
                    f(IVec3::new(x, y, z));
                }
            }
        }
    }

    /// Lancer de rayon dans la grille (DDA, en unités de cellules).
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<RayHit> {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return None;
        }

        let local = (origin - self.origin) / VOXEL;
        let pos = [local.x, local.y, local.z];
        let d = [dir.x, dir.y, dir.z];
        let mut cell = [0i32; 3];
        let mut step = [0i32; 3];
        let mut t_max = [0.0f32; 3];
        let mut t_delta = [0.0f32; 3];

        for a in 0..3 {
            cell[a] = pos[a].floor() as i32;
            if d[a] > 0.0 {
                step[a] = 1;
                t_delta[a] = 1.0 / d[a];
                t_max[a] = (cell[a] as f32 + 1.0 - pos[a]) / d[a];
            } else if d[a] < 0.0 {
                step[a] = -1;
                t_delta[a] = -1.0 / d[a];
                t_max[a] = (cell[a] as f32 - pos[a]) / d[a];
            } else {
                step[a] = 0;
                t_delta[a] = f32::INFINITY;
                t_max[a] = f32::INFINITY;
            }
        }

        let max_cells = max_dist / VOXEL;
        let mut t = 0.0f32;
        let mut normal = IVec3::ZERO;

        loop {
            if self.is_solid(IVec3::new(cell[0], cell[1], cell[2])) {
                return Some(RayHit {
                    point: origin + dir * (t * VOXEL),
                    normal: normal.as_vec3(),
                });
            }

            let a = if t_max[0] <= t_max[1] && t_max[0] <= t_max[2] {
                0
            } else if t_max[1] <= t_max[2] {
                1
            } else {
                2
            };
            let next = t_max[a];
            if next > max_cells || step[a] == 0 {
                return None;
            }

            cell[a] += step[a];
            t = next;
            t_max[a] += t_delta[a];
            normal = IVec3::new(
                if a == 0 { -step[a] } else { 0 },
                if a == 1 { -step[a] } else { 0 },
                if a == 2 { -step[a] } else { 0 },
            );
        }
    }

    /// Détruit tous les voxels d'une sphère. Renvoie les cellules touchées.
    ///
    /// Le rendu se met à jour via `dirty` : plus aucune entité à déspawner,
    /// le client comme le serveur utilisent exactement ce code.
    pub fn carve(&mut self, center: Vec3, radius: f32) -> Vec<IVec3> {
        let r = Vec3::splat(radius);
        let c0 = self.world_to_cell(center - r);
        let c1 = self.world_to_cell(center + r);
        let radius_sq = radius * radius;
        let mut destroyed = Vec::new();

        for z in c0.z.max(0)..=c1.z.min(self.dims.z - 1) {
            for y in c0.y.max(0)..=c1.y.min(self.dims.y - 1) {
                for x in c0.x.max(0)..=c1.x.min(self.dims.x - 1) {
                    let c = IVec3::new(x, y, z);
                    if !self.is_solid(c) {
                        continue;
                    }
                    let d = self.cell_center(c) - center;
                    if d.length_squared() > radius_sq {
                        continue;
                    }
                    self.set(c, false, 0);
                    destroyed.push(c);
                }
            }
        }
        destroyed
    }

    /// Déplace un solide de `delta` sur un axe et le repousse hors des voxels.
    ///
    /// Retourne `true` si un contact a eu lieu (l'appelant annule alors la
    /// vitesse sur cet axe). `half` est la demi-taille du solide, ce qui permet
    /// de réutiliser ça aussi bien pour les blocs de gravats que pour les
    /// débris ou le joueur.
    pub fn sweep(&self, pos: &mut Vec3, half: Vec3, axis: usize, delta: f32) -> bool {
        set_component(pos, axis, get_component(*pos, axis) + delta);
        let min = *pos - half;
        let max = *pos + half;

        let mut bound: Option<f32> = None;
        self.visit_cells(min, max, |c| {
            if !self.is_solid(c) {
                return;
            }
            let cmin = get_component(self.cell_min(c), axis);
            let face = if delta < 0.0 { cmin + VOXEL } else { cmin };
            bound = Some(match bound {
                None => face,
                Some(b) if delta < 0.0 => b.max(face),
                Some(b) => b.min(face),
            });
        });

        match bound {
            Some(face) => {
                set_component(
                    pos,
                    axis,
                    face + if delta < 0.0 { half[axis] } else { -half[axis] },
                );
                true
            }
            None => false,
        }
    }

    /// (Re)construit la carte complète.
    pub fn rebuild(&mut self) {
        self.clear();
        self.dirty_box = None;
        crate::map::generate(self);
        self.total = self.alive;
        self.dirty = true;
    }
}

// ---------------------------------------------------------------------------
// Aides de construction (utilisées par `map`)
// ---------------------------------------------------------------------------

/// Composante d'un vecteur selon son axe.
#[inline]
pub fn get_component(v: Vec3, axis: usize) -> f32 {
    match axis {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

/// Écrit la composante d'un vecteur selon son axe.
#[inline]
pub fn set_component(v: &mut Vec3, axis: usize, value: f32) {
    match axis {
        0 => v.x = value,
        1 => v.y = value,
        _ => v.z = value,
    }
}

impl VoxelWorld {
    /// Remplit une boîte (coin min inclus, coin max exclus).
    pub fn fill(&mut self, min: IVec3, max: IVec3, material: u8) {
        for z in min.z..max.z {
            for y in min.y..max.y {
                for x in min.x..max.x {
                    let c = IVec3::new(x, y, z);
                    if self.index(c).is_some() {
                        self.set(c, true, material);
                    }
                }
            }
        }
    }

    /// Murs d'une boîte creuse (é美观paisseur 1), avec ouvertures.
    pub fn shell(&mut self, min: IVec3, max: IVec3, material: u8, holes: &[IVec3]) {
        for z in min.z..max.z {
            for y in min.y..max.y {
                for x in min.x..max.x {
                    let c = IVec3::new(x, y, z);
                    if self.index(c).is_none() {
                        continue;
                    }
                    let on_wall = x == min.x
                        || x == max.x - 1
                        || z == min.z
                        || z == max.z - 1
                        || y == min.y
                        || y == max.y - 1;
                    if !on_wall {
                        continue;
                    }
                    // Ouvertures : une cellule listée ici est retirée.
                    if holes.iter().any(|h| {
                        c.x >= h.x && c.x < h.x + h.z && c.y >= h.y && c.y < h.y + h.z
                    }) {
                        continue;
                    }
                    self.set(c, true, material);
                }
            }
        }
    }
}

/// Mesh cube et palette de matériaux partagés.
#[derive(Resource)]
pub struct VoxelAssets {
    pub cube: Handle<Mesh>,
    pub debris_cube: Handle<Mesh>,
    /// Cube unitaire mis à l'échelle : blocs de gravats.
    pub chunk_cube: Handle<Mesh>,
    /// Matériaux (indice = matériau de la cellule).
    pub materials: Vec<Handle<StandardMaterial>>,
    /// Matériau unique du maillage fusionné (blanc : la couleur vient des
    /// sommets).
    pub voxel_material: Handle<StandardMaterial>,
    /// Traînée de balle.
    pub tracer_cube: Handle<Mesh>,
    pub tracer_material: Handle<StandardMaterial>,
}

/// Palette : couleur par identifiant de matériau (sert aux sommets).
pub const PALETTE: [(f32, f32, f32); 12] = [
    (0.55, 0.55, 0.52), // 0 béton
    (0.34, 0.34, 0.33), // 1 béton sombre
    (0.20, 0.20, 0.21), // 2 asphalte
    (0.30, 0.44, 0.20), // 3 herbe
    (0.62, 0.55, 0.40), // 4 terre / sable
    (0.62, 0.29, 0.21), // 5 brique
    (0.48, 0.22, 0.16), // 6 brique sombre
    (0.72, 0.70, 0.65), // 7 mortier
    (0.52, 0.36, 0.18), // 8 bois
    (0.45, 0.47, 0.50), // 9 métal
    (0.55, 0.72, 0.82), // 10 verre
    (0.80, 0.78, 0.72), // 11 plâtre
];

/// Identifiants de matériaux.
pub mod mat {
    pub const CONCRETE: u8 = 0;
    pub const CONCRETE_DARK: u8 = 1;
    pub const ASPHALT: u8 = 2;
    pub const GRASS: u8 = 3;
    pub const DIRT: u8 = 4;
    pub const BRICK: u8 = 5;
    pub const BRICK_DARK: u8 = 6;
    pub const MORTAR: u8 = 7;
    pub const WOOD: u8 = 8;
    pub const METAL: u8 = 9;
    pub const GLASS: u8 = 10;
    pub const PLASTER: u8 = 11;
}

/// Crée les meshes et la palette.
pub fn setup_voxel_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let make = |rgb: (f32, f32, f32)| {
        materials.add(StandardMaterial {
            base_color: Color::srgb(rgb.0, rgb.1, rgb.2),
            perceptual_roughness: 0.92,
            ..default()
        })
    };
    let palette: Vec<_> = PALETTE.iter().copied().map(make).collect();

    commands.insert_resource(VoxelAssets {
        cube: meshes.add(Cuboid::new(VOXEL, VOXEL, VOXEL)),
        debris_cube: meshes.add(Cuboid::new(VOXEL * 0.45, VOXEL * 0.45, VOXEL * 0.45)),
        chunk_cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        tracer_cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        tracer_material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.85, 0.45),
            emissive: Color::srgb(1.0, 0.75, 0.3).into(),
            unlit: true,
            ..default()
        }),
        voxel_material: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.9,
            ..default()
        }),
        materials: palette,
    });
}

/// Reconstruit la carte au démarrage.
pub fn build_level(mut world: ResMut<VoxelWorld>) {
    world.rebuild();
}

/// `F5` reconstruit le niveau.
pub fn handle_reset(keys: Res<ButtonInput<KeyCode>>, mut world: ResMut<VoxelWorld>) {
    if keys.just_pressed(KeyCode::F5) {
        world.rebuild();
    }
}
/// Destructeur commun (masse, balle, explosion) : creuse la sphère, crée les
/// blocs de gravats et quelques débris.
#[allow(clippy::too_many_arguments)]
pub fn damage(
    world: &mut VoxelWorld,
    chunks: &mut crate::rubble::Chunks,
    commands: &mut Commands,
    assets: &VoxelAssets,
    rng: &mut crate::debris::Rng,
    center: Vec3,
    radius: f32,
    max_debris: usize,
    push: Vec3,
) -> u32 {
    let destroyed = world.carve(center, radius);
    let count = destroyed.len();
    for &cell in &destroyed {
        crate::rubble::break_voxel(chunks, commands, assets, world, cell, center, rng);
    }

    let debris = count.min(max_debris);
    for _ in 0..debris {
        let cell = destroyed[rng.usize(destroyed.len())];
        let pos = world.cell_center(cell);
        let material = world.material(cell);
        let away = (pos - center).normalize_or_zero();
        let vel = away * (2.0 + rng.f32() * 3.0)
            + push * (1.0 + rng.f32() * 2.0)
            + Vec3::new(rng.signed(), rng.signed(), rng.signed()) * 2.0;
        crate::debris::spawn_debris(commands, assets, rng, pos, material, vel);
    }

    count as u32
}
