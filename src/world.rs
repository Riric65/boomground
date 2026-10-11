//! Le monde destructible : une grille de voxels compacte.
//!
//! Stockage : un **bitset** pour l'occupation (1 bit par cellule) et un
//! octet pour le matériau. Le rendu passe par des maillages fusionnés par
//! morceaux (voir `vmesh`), donc aucune entité par voxel.
//!
//! La carte est générée à partir d'une **seed** (voir `map`) : la même seed
//! donne toujours la même ville, ce qui permet aussi au serveur et aux
//! clients d'avoir exactement le même monde.

use std::collections::HashSet;

use bevy::prelude::*;

/// Taille d'un voxel, en mètres.
pub const VOXEL: f32 = 0.4;

/// Dimensions de la grille (X, Y, Z) en cellules : 153 × 25 × 153 m.
pub const GRID: [i32; 3] = [384, 64, 384];

/// Côté d'un morceau de rendu, en cellules (voir `vmesh`).
pub const MESH_CHUNK: i32 = 16;

/// Pas maximal d'un déplacement avant de retester les collisions : moins
/// d'un demi-voxel, sinon un objet rapide traverse un mur fin.
const MAX_STEP: f32 = VOXEL * 0.45;

/// Résultat d'un lancer de rayon.
pub struct RayHit {
    pub point: Vec3,
    /// Normale de la face touchée (nulle si le rayon démarre dans un voxel).
    pub normal: Vec3,
    /// Cellule touchée.
    pub cell: IVec3,
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
    /// Seed de la carte actuelle.
    pub seed: u64,
    /// Point d'apparition calculé par le générateur.
    pub spawn: Vec3,
    /// Incrémenté à chaque reconstruction complète (les systèmes s'en
    /// servent pour vider gravats, débris, C4…).
    pub generation: u32,
    /// Toute la carte doit être remaillée.
    pub full_rebuild: bool,
    /// Morceaux de rendu à reconstruire.
    dirty_chunks: HashSet<IVec3>,
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
            seed: 0,
            spawn: Vec3::new(0.0, 4.0, 0.0),
            generation: 0,
            full_rebuild: true,
            dirty_chunks: HashSet::new(),
        }
    }

    /// Morceaux de rendu à reconstruire (vidés par l'appelant).
    pub fn dirty_chunks_mut(&mut self) -> &mut HashSet<IVec3> {
        &mut self.dirty_chunks
    }

    /// Marque les morceaux de rendu touchés par un changement de `c`.
    ///
    /// Le plan entre la cellule `c - 1` et `c` appartient au morceau de
    /// `c - 1` : sur un bord bas, le voisin doit donc être reconstruit aussi.
    fn touch(&mut self, c: IVec3) {
        if self.full_rebuild {
            return;
        }
        let key = c.div_euclid(IVec3::splat(MESH_CHUNK));
        self.dirty_chunks.insert(key);
        for axis in 0..3 {
            if c[axis].rem_euclid(MESH_CHUNK) == 0 && c[axis] > 0 {
                let mut k = key;
                k[axis] -= 1;
                self.dirty_chunks.insert(k);
            }
        }
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

    /// Matériau de la cellule. Reste lisible après destruction (les gravats
    /// s'en servent pour leur couleur).
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
        } else if !solid {
            return;
        }
        if solid {
            self.mat[i] = material;
        }
        self.touch(c);
    }

    /// Vide toute la grille.
    pub fn clear(&mut self) {
        self.solid.iter_mut().for_each(|w| *w = 0);
        self.mat.iter_mut().for_each(|m| *m = 0);
        self.alive = 0;
        self.total = 0;
        self.dirty_chunks.clear();
        self.full_rebuild = true;
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

    /// Le monde, une fois sorti de la grille, n'a plus de sol.
    pub fn in_bounds_xz(&self, p: Vec3) -> bool {
        let c = self.world_to_cell(p);
        c.x >= 0 && c.z >= 0 && c.x < self.dims.x && c.z < self.dims.z
    }

    /// Vrai si une boîte `[min, max]` chevauche au moins un voxel plein.
    pub fn overlaps(&self, min: Vec3, max: Vec3) -> bool {
        const EPS: f32 = 1e-3;
        let c0 = self.world_to_cell(min + Vec3::splat(EPS));
        let c1 = self.world_to_cell(max - Vec3::splat(EPS));
        for x in c0.x..=c1.x {
            for y in c0.y..=c1.y {
                for z in c0.z..=c1.z {
                    if self.is_solid(IVec3::new(x, y, z)) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Hauteur (monde) du dessus du voxel le plus haut sous `(x, z)`.
    pub fn ground_height(&self, x: f32, z: f32) -> f32 {
        let c = self.world_to_cell(Vec3::new(x, 0.0, z));
        for y in (0..self.dims.y).rev() {
            if self.is_solid(IVec3::new(c.x, y, c.z)) {
                return self.cell_max(IVec3::new(c.x, y, c.z)).y;
            }
        }
        0.0
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
            let c = IVec3::new(cell[0], cell[1], cell[2]);
            if self.is_solid(c) {
                return Some(RayHit {
                    point: origin + dir * (t * VOXEL),
                    normal: normal.as_vec3(),
                    cell: c,
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
            normal = IVec3::ZERO;
            normal[a] = -step[a];
        }
    }

    /// Détruit tous les voxels d'une sphère. Renvoie les cellules touchées.
    ///
    /// La couche du fond (`y = 0`) est indestructible : sans elle on finirait
    /// par tomber hors du monde.
    pub fn carve(&mut self, center: Vec3, radius: f32) -> Vec<IVec3> {
        let r = Vec3::splat(radius);
        let c0 = self.world_to_cell(center - r);
        let c1 = self.world_to_cell(center + r);
        let radius_sq = radius * radius;
        let mut destroyed = Vec::new();

        for z in c0.z.max(0)..=c1.z.min(self.dims.z - 1) {
            for y in c0.y.max(1)..=c1.y.min(self.dims.y - 1) {
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

    /// Déplace une boîte centrée en `pos` (demi-taille `half`) de `delta`
    /// sur un axe, en s'arrêtant contre le premier voxel rencontré.
    ///
    /// Seuls les voxels **devant** la face avant de la boîte comptent : un
    /// objet qui chevauche déjà un voxel (gravats qui apparaissent dans un
    /// mur, joueur poussé) n'est jamais téléporté de l'autre côté, il peut
    /// simplement s'en dégager. Le déplacement est découpé en petits pas pour
    /// ne jamais traverser un mur fin. Renvoie `true` en cas de contact.
    pub fn sweep(&self, pos: &mut Vec3, half: Vec3, axis: usize, delta: f32) -> bool {
        self.sweep_with(pos, half, axis, delta, &[])
    }

    /// Comme [`Self::sweep`], avec en plus des boîtes solides (gravats)
    /// données sous forme `(min, max)`.
    pub fn sweep_with(
        &self,
        pos: &mut Vec3,
        half: Vec3,
        axis: usize,
        delta: f32,
        boxes: &[(Vec3, Vec3)],
    ) -> bool {
        let mut remaining = delta;
        while remaining != 0.0 {
            let step = remaining.clamp(-MAX_STEP, MAX_STEP);
            remaining -= step;
            if self.sweep_step(pos, half, axis, step, boxes) {
                return true;
            }
        }
        false
    }

    fn sweep_step(
        &self,
        pos: &mut Vec3,
        half: Vec3,
        axis: usize,
        step: f32,
        boxes: &[(Vec3, Vec3)],
    ) -> bool {
        const EPS: f32 = 1e-4;
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let sign = step.signum();
        let lead_old = pos[axis] + sign * half[axis];
        let lead_new = lead_old + step;

        // Plage de cellules sur les deux autres axes.
        let lo = self.world_to_cell(*pos - half + Vec3::splat(EPS));
        let hi = self.world_to_cell(*pos + half - Vec3::splat(EPS));

        let o = self.origin[axis];
        let x_old = (lead_old - o) / VOXEL;
        let x_new = (lead_new - o) / VOXEL;

        // Plan de contact le plus proche (voxels puis boîtes).
        let mut contact: Option<f32> = None;

        if step > 0.0 {
            let i0 = (x_old - 1e-3).ceil() as i32;
            let i1 = x_new.ceil() as i32 - 1;
            'pos: for i in i0..=i1 {
                for a in lo[u]..=hi[u] {
                    for b in lo[v]..=hi[v] {
                        let mut c = IVec3::ZERO;
                        c[axis] = i;
                        c[u] = a;
                        c[v] = b;
                        if self.is_solid(c) {
                            contact = Some(o + i as f32 * VOXEL);
                            break 'pos;
                        }
                    }
                }
            }
        } else {
            let i0 = (x_old + 1e-3).floor() as i32 - 1;
            let i1 = x_new.floor() as i32;
            'neg: for i in (i1..=i0).rev() {
                for a in lo[u]..=hi[u] {
                    for b in lo[v]..=hi[v] {
                        let mut c = IVec3::ZERO;
                        c[axis] = i;
                        c[u] = a;
                        c[v] = b;
                        if self.is_solid(c) {
                            contact = Some(o + (i + 1) as f32 * VOXEL);
                            break 'neg;
                        }
                    }
                }
            }
        }

        // Boîtes (gravats) : même règle, seules celles devant comptent.
        for &(bmin, bmax) in boxes {
            let overlap_u = pos[u] + half[u] > bmin[u] + EPS && pos[u] - half[u] < bmax[u] - EPS;
            let overlap_v = pos[v] + half[v] > bmin[v] + EPS && pos[v] - half[v] < bmax[v] - EPS;
            if !overlap_u || !overlap_v {
                continue;
            }
            if step > 0.0 {
                let face = bmin[axis];
                if face >= lead_old - 1e-3 && face < lead_new {
                    contact = Some(contact.map_or(face, |c| c.min(face)));
                }
            } else {
                let face = bmax[axis];
                if face <= lead_old + 1e-3 && face > lead_new {
                    contact = Some(contact.map_or(face, |c| c.max(face)));
                }
            }
        }

        match contact {
            Some(face) => {
                pos[axis] = face - sign * half[axis];
                true
            }
            None => {
                pos[axis] += step;
                false
            }
        }
    }

    /// (Re)construit la carte complète à partir d'une seed.
    pub fn rebuild(&mut self, seed: u64) {
        self.clear();
        self.seed = seed;
        self.spawn = crate::map::generate(self, seed);
        self.total = self.alive;
        self.generation = self.generation.wrapping_add(1);
        self.full_rebuild = true;
    }
}

/// Composante d'un vecteur selon son axe.
#[inline]
pub fn get_component(v: Vec3, axis: usize) -> f32 {
    v[axis]
}

/// Écrit la composante d'un vecteur selon son axe.
#[inline]
pub fn set_component(v: &mut Vec3, axis: usize, value: f32) {
    v[axis] = value;
}

impl VoxelWorld {
    /// Remplit une boîte (coin min inclus, coin max exclus).
    pub fn fill(&mut self, min: IVec3, max: IVec3, material: u8) {
        let min = min.max(IVec3::ZERO);
        let max = max.min(self.dims);
        for z in min.z..max.z {
            for y in min.y..max.y {
                for x in min.x..max.x {
                    self.set(IVec3::new(x, y, z), true, material);
                }
            }
        }
    }

    /// Vide une boîte (coin min inclus, coin max exclus).
    pub fn empty(&mut self, min: IVec3, max: IVec3) {
        let min = min.max(IVec3::ZERO);
        let max = max.min(self.dims);
        for z in min.z..max.z {
            for y in min.y..max.y {
                for x in min.x..max.x {
                    self.set(IVec3::new(x, y, z), false, 0);
                }
            }
        }
    }
}

/// Mesh cube et palette de matériaux partagés.
#[derive(Resource)]
pub struct VoxelAssets {
    pub debris_cube: Handle<Mesh>,
    /// Cube unitaire mis à l'échelle : blocs de gravats.
    pub chunk_cube: Handle<Mesh>,
    /// Matériaux (indice = matériau de la cellule).
    pub materials: Vec<Handle<StandardMaterial>>,
    /// Matériau unique du maillage fusionné (blanc : la couleur vient des
    /// sommets).
    pub voxel_material: Handle<StandardMaterial>,
    /// Vitres : maillage séparé, translucide.
    pub glass_material: Handle<StandardMaterial>,
    /// Traînée de balle.
    pub tracer_cube: Handle<Mesh>,
    pub tracer_material: Handle<StandardMaterial>,
}

/// Palette (sRGB) : couleur par identifiant de matériau.
pub const PALETTE: [(f32, f32, f32); 16] = [
    (0.58, 0.58, 0.55), // 0 béton
    (0.36, 0.36, 0.35), // 1 béton sombre
    (0.17, 0.17, 0.18), // 2 asphalte
    (0.33, 0.50, 0.22), // 3 herbe
    (0.45, 0.34, 0.22), // 4 terre
    (0.64, 0.30, 0.22), // 5 brique
    (0.48, 0.22, 0.16), // 6 brique sombre
    (0.76, 0.74, 0.68), // 7 mortier
    (0.55, 0.38, 0.20), // 8 bois
    (0.50, 0.53, 0.57), // 9 métal
    (0.62, 0.80, 0.90), // 10 verre
    (0.84, 0.82, 0.76), // 11 plâtre
    (0.12, 0.12, 0.13), // 12 socle (indestructible)
    (0.24, 0.45, 0.18), // 13 feuillage
    (0.88, 0.86, 0.30), // 14 peinture (lignes de route)
    (0.70, 0.18, 0.15), // 15 tuile / métal peint
];

/// Couleur linéaire d'un matériau : les couleurs de sommets sont lues en
/// linéaire, alors que `Color::srgb` convertit. Sans ça, les murs et leurs
/// gravats n'avaient pas la même teinte.
pub fn palette_linear(material: u8) -> [f32; 4] {
    let (r, g, b) = PALETTE[material as usize % PALETTE.len()];
    let c = Color::srgb(r, g, b).to_linear();
    [c.red, c.green, c.blue, 1.0]
}

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
    pub const BEDROCK: u8 = 12;
    pub const LEAVES: u8 = 13;
    pub const PAINT: u8 = 14;
    pub const ROOF: u8 = 15;
}

/// Argent rapporté par un voxel détruit (mode normal). Le sol ne rapporte
/// rien : sinon il suffirait de creuser pour devenir riche.
pub fn material_value(material: u8) -> u32 {
    use mat::*;
    match material {
        BRICK | BRICK_DARK | GLASS | ROOF => 2,
        METAL => 3,
        CONCRETE | CONCRETE_DARK | MORTAR | WOOD | PLASTER => 1,
        _ => 0,
    }
}

/// Crée les meshes et la palette.
pub fn setup_voxel_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let palette: Vec<_> = (0..PALETTE.len())
        .map(|i| {
            let (r, g, b) = PALETTE[i];
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                perceptual_roughness: 0.92,
                ..default()
            })
        })
        .collect();

    commands.insert_resource(VoxelAssets {
        debris_cube: meshes.add(Cuboid::new(VOXEL * 0.45, VOXEL * 0.45, VOXEL * 0.45)),
        chunk_cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        tracer_cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        tracer_material: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.85, 0.45),
            emissive: Color::srgb(4.0, 3.0, 1.2).into(),
            unlit: true,
            ..default()
        }),
        voxel_material: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.9,
            ..default()
        }),
        glass_material: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 1.0, 1.0, 0.45),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.08,
            reflectance: 0.6,
            ..default()
        }),
        materials: palette,
    });
}

/// Construit la carte au démarrage (seed aléatoire, le menu la remplace).
pub fn build_level(mut world: ResMut<VoxelWorld>, mut game: ResMut<crate::game::GameState>) {
    if game.seed == 0 {
        game.seed = crate::game::random_seed();
    }
    world.rebuild(game.seed);
}

/// `F5` reconstruit la même ville, `F6` en tire une nouvelle (solo).
pub fn handle_reset(
    keys: Res<ButtonInput<KeyCode>>,
    mut world: ResMut<VoxelWorld>,
    mut game: ResMut<crate::game::GameState>,
    net: Res<crate::net::client::NetClient>,
) {
    // En multijoueur, la carte appartient au serveur.
    if net.is_online() {
        return;
    }
    if keys.just_pressed(KeyCode::F5) {
        world.rebuild(game.seed);
    }
    if keys.just_pressed(KeyCode::F6) {
        game.seed = crate::game::random_seed();
        world.rebuild(game.seed);
    }
}

/// Résultat d'une destruction : nombre de voxels, argent gagné, vitre brisée.
pub struct Damage {
    pub count: u32,
    pub value: u32,
    pub glass: bool,
}

/// Destructeur commun (masse, balle, explosion) : creuse la sphère, crée les
/// blocs de gravats et quelques débris.
#[allow(clippy::too_many_arguments)]
pub fn damage(
    world: &mut VoxelWorld,
    rubble: &mut crate::rubble::Rubble,
    commands: &mut Commands,
    assets: &VoxelAssets,
    rng: &mut crate::debris::Rng,
    center: Vec3,
    radius: f32,
    max_debris: usize,
    push: Vec3,
) -> Damage {
    let destroyed = world.carve(center, radius);
    let value = destroyed
        .iter()
        .map(|&c| material_value(world.material(c)))
        .sum();
    crate::rubble::spawn_rubble(rubble, commands, assets, world, &destroyed, center, push, rng);

    let debris = destroyed.len().min(max_debris);
    for _ in 0..debris {
        let cell = destroyed[rng.usize(destroyed.len())];
        let pos = world.cell_center(cell);
        let material = world.material(cell);
        let away = (pos - center).normalize_or_zero();
        let vel = away * (2.0 + rng.f32() * 3.0)
            + push * (0.6 + rng.f32() * 1.0)
            + Vec3::new(rng.signed(), rng.signed(), rng.signed()) * 2.0;
        crate::debris::spawn_debris(commands, assets, rng, pos, material, vel);
    }

    Damage {
        count: destroyed.len() as u32,
        value,
        glass: destroyed.iter().any(|&c| world.material(c) == mat::GLASS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world_with_wall() -> VoxelWorld {
        let mut w = VoxelWorld::new();
        // Mur d'un voxel d'épaisseur en x = 100.
        w.fill(IVec3::new(100, 0, 90), IVec3::new(101, 20, 110), mat::BRICK);
        w
    }

    #[test]
    fn fast_box_does_not_tunnel_through_thin_wall() {
        let w = world_with_wall();
        let wall_x = w.cell_min(IVec3::new(100, 0, 0)).x;
        let mut pos = Vec3::new(wall_x - 1.0, w.cell_min(IVec3::new(0, 5, 0)).y, w.cell_center(IVec3::new(0, 0, 100)).z);
        let hit = w.sweep(&mut pos, Vec3::splat(0.3), 0, 5.0);
        assert!(hit);
        assert!((pos.x + 0.3 - wall_x).abs() < 1e-3, "arrêt contre la face : {pos}");
    }

    #[test]
    fn box_inside_wall_is_not_teleported_through() {
        let w = world_with_wall();
        let wall_x = w.cell_min(IVec3::new(100, 0, 0)).x;
        // Déjà à moitié dans le mur : avancer ne doit pas le projeter de
        // l'autre côté, reculer doit le libérer.
        let mut pos = Vec3::new(wall_x + 0.1, w.cell_min(IVec3::new(0, 5, 0)).y, w.cell_center(IVec3::new(0, 0, 100)).z);
        let start = pos;
        w.sweep(&mut pos, Vec3::splat(0.3), 0, -0.5);
        assert!(pos.x < start.x);
    }

    #[test]
    fn carve_keeps_bedrock() {
        let mut w = VoxelWorld::new();
        w.fill(IVec3::ZERO, IVec3::new(4, 2, 4), mat::BEDROCK);
        let destroyed = w.carve(w.cell_center(IVec3::new(1, 0, 1)), 2.0);
        assert!(destroyed.iter().all(|c| c.y > 0));
        assert!(w.is_solid(IVec3::new(1, 0, 1)));
    }

    #[test]
    fn same_seed_same_map() {
        let mut a = VoxelWorld::new();
        let mut b = VoxelWorld::new();
        a.rebuild(1234);
        b.rebuild(1234);
        assert_eq!(a.alive, b.alive);
        assert_eq!(a.spawn, b.spawn);
        let mut c = VoxelWorld::new();
        c.rebuild(98765);
        assert_ne!(a.alive, c.alive);
    }
}
