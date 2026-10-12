//! Le monde destructible : une grille de voxels compacte.
//!
//! Stockage : des **morceaux** de 16³ cellules alloués à la demande (un
//! bitset d'occupation et un octet de matériau par cellule). Le ciel vide
//! ne coûte rien, ce qui permet une très grande carte. Le rendu passe par
//! des maillages fusionnés par morceaux (voir `vmesh`), donc aucune entité
//! par voxel.
//!
//! La carte est générée à partir d'une **seed** (voir `map`) : la même seed
//! donne toujours la même ville, ce qui permet aussi au serveur et aux
//! clients d'avoir exactement le même monde.

use std::collections::HashSet;

use bevy::prelude::*;

/// Taille d'un voxel, en mètres.
pub const VOXEL: f32 = 0.4;

/// Dimensions de la grille (X, Y, Z) en cellules : 845 × 25 × 845 m
/// (33 × 33 pâtés de maisons, soit ~30 fois l'ancienne carte).
pub const GRID: [i32; 3] = [2112, 64, 2112];

/// Taille par défaut de la carte : 33 × 33 pâtés de maisons.
pub const DEFAULT_BLOCKS: i32 = 33;
/// Plus petite carte : 5 × 5 pâtés.
pub const MIN_BLOCKS: i32 = 5;

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

/// Cellules par morceau de stockage : `MESH_CHUNK`³.
const CHUNK_CELLS: usize = (MESH_CHUNK * MESH_CHUNK * MESH_CHUNK) as usize;

/// Matériaux d'un morceau : palette de 16 entrées max et 4 bits par
/// cellule (2 Ko au lieu de 4 Ko), avec repli sur 8 bits si un morceau
/// mélange plus de 16 matériaux.
enum MatStore {
    Narrow { pal: [u8; 16], len: u8, idx: Box<[u8; CHUNK_CELLS / 2]> },
    Wide(Box<[u8; CHUNK_CELLS]>),
}

impl MatStore {
    #[inline]
    fn get(&self, i: usize) -> u8 {
        match self {
            MatStore::Narrow { pal, idx, .. } => {
                let b = idx[i >> 1];
                pal[(if i & 1 == 0 { b & 15 } else { b >> 4 }) as usize]
            }
            MatStore::Wide(w) => w[i],
        }
    }

    fn set(&mut self, i: usize, m: u8) {
        match self {
            MatStore::Narrow { pal, len, idx } => {
                let found = pal[..*len as usize].iter().position(|&p| p == m);
                let slot = match found {
                    Some(k) => k as u8,
                    None if *len < 16 => {
                        pal[*len as usize] = m;
                        *len += 1;
                        *len - 1
                    }
                    None => {
                        // Palette pleine : on passe en 8 bits.
                        let mut wide = Box::new([0u8; CHUNK_CELLS]);
                        for (c, cell) in wide.iter_mut().enumerate() {
                            let b = idx[c >> 1];
                            *cell = pal[(if c & 1 == 0 { b & 15 } else { b >> 4 }) as usize];
                        }
                        wide[i] = m;
                        *self = MatStore::Wide(wide);
                        return;
                    }
                };
                let b = &mut idx[i >> 1];
                if i & 1 == 0 {
                    *b = (*b & 0xF0) | slot;
                } else {
                    *b = (*b & 0x0F) | (slot << 4);
                }
            }
            MatStore::Wide(w) => w[i] = m,
        }
    }
}

/// Un morceau de stockage (alloué seulement s'il contient du solide).
struct Chunk {
    solid: [u64; CHUNK_CELLS / 64],
    mat: MatStore,
}

impl Chunk {
    fn new() -> Box<Self> {
        Box::new(Self {
            solid: [0; CHUNK_CELLS / 64],
            // Entrée 0 = matériau 0 : un morceau neuf lit 0 partout.
            mat: MatStore::Narrow {
                pal: [0; 16],
                len: 1,
                idx: Box::new([0; CHUNK_CELLS / 2]),
            },
        })
    }
}

#[derive(Resource)]
pub struct VoxelWorld {
    pub dims: IVec3,
    /// Côté de la carte, en pâtés de maisons.
    pub blocks: i32,
    /// Coin « bas » monde de la cellule (0, 0, 0).
    pub origin: Vec3,
    /// Nombre de morceaux par axe.
    chunk_dims: IVec3,
    /// Morceaux de stockage (`None` = entièrement vide).
    chunks: Vec<Option<Box<Chunk>>>,
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
    /// Portes (interactives) générées avec la carte.
    pub doors: Vec<Door>,
}

/// Une porte : ses voxels passent de `closed` à `open` (et inversement),
/// cellule par cellule. Une cellule détruite reste détruite.
pub struct Door {
    pub closed: Vec<IVec3>,
    pub open: Vec<IVec3>,
    pub is_open: bool,
    /// Nombre de voxels d'origine (une porte à moitié détruite ne bouge plus).
    pub total: usize,
}

impl Door {
    /// Centre actuel de la porte (monde).
    pub fn center(&self, w: &VoxelWorld) -> Vec3 {
        let cells = if self.is_open { &self.open } else { &self.closed };
        let sum: Vec3 = cells.iter().map(|&c| w.cell_center(c)).sum();
        sum / cells.len().max(1) as f32
    }
}

impl Default for VoxelWorld {
    fn default() -> Self {
        Self::new()
    }
}

impl VoxelWorld {
    pub fn new() -> Self {
        Self::with_blocks(DEFAULT_BLOCKS)
    }

    /// Carte de `blocks` × `blocks` pâtés de maisons (64 voxels chacun).
    pub fn with_blocks(blocks: i32) -> Self {
        let blocks = blocks.clamp(MIN_BLOCKS, DEFAULT_BLOCKS * 2);
        let dims = IVec3::new(blocks * 64, GRID[1], blocks * 64);
        let chunk_dims = dims / MESH_CHUNK;
        let count = (chunk_dims.x * chunk_dims.y * chunk_dims.z) as usize;
        let size = dims.as_vec3() * VOXEL;
        Self {
            dims,
            origin: Vec3::new(-size.x * 0.5, 0.0, -size.z * 0.5),
            chunk_dims,
            chunks: (0..count).map(|_| None).collect(),
            blocks,
            alive: 0,
            total: 0,
            seed: 0,
            spawn: Vec3::new(0.0, 4.0, 0.0),
            generation: 0,
            full_rebuild: true,
            dirty_chunks: HashSet::new(),
            doors: Vec::new(),
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
        // Une cellule modifie les faces des cellules voisines (jusqu'à une
        // cellule de distance : occlusion ambiante comprise), donc les
        // morceaux voisins quand elle touche leur bord.
        let key = c.div_euclid(IVec3::splat(MESH_CHUNK));
        let local = c.rem_euclid(IVec3::splat(MESH_CHUNK));
        let mut lo = [0i32; 3];
        let mut hi = [0i32; 3];
        for a in 0..3 {
            if local[a] == 0 {
                lo[a] = -1;
            }
            if local[a] == MESH_CHUNK - 1 {
                hi[a] = 1;
            }
        }
        for dx in lo[0]..=hi[0] {
            for dy in lo[1]..=hi[1] {
                for dz in lo[2]..=hi[2] {
                    let k = key + IVec3::new(dx, dy, dz);
                    if k.cmpge(IVec3::ZERO).all() {
                        self.dirty_chunks.insert(k);
                    }
                }
            }
        }
    }

    /// (indice du morceau, indice dans le morceau) d'une cellule.
    #[inline]
    fn locate(&self, c: IVec3) -> Option<(usize, usize)> {
        if c.x < 0 || c.y < 0 || c.z < 0 || c.x >= self.dims.x || c.y >= self.dims.y || c.z >= self.dims.z
        {
            return None;
        }
        let k = c / MESH_CHUNK;
        let l = c & (MESH_CHUNK - 1);
        let chunk = (k.x * self.chunk_dims.y + k.y) * self.chunk_dims.z + k.z;
        let local = (l.x * MESH_CHUNK + l.y) * MESH_CHUNK + l.z;
        Some((chunk as usize, local as usize))
    }

    #[inline]
    pub fn is_solid(&self, c: IVec3) -> bool {
        match self.locate(c) {
            Some((k, i)) => match &self.chunks[k] {
                Some(ch) => ch.solid[i >> 6] & (1u64 << (i & 63)) != 0,
                None => false,
            },
            None => false,
        }
    }

    /// Matériau de la cellule. Reste lisible après destruction (les gravats
    /// s'en servent pour leur couleur).
    #[inline]
    pub fn material(&self, c: IVec3) -> u8 {
        match self.locate(c) {
            Some((k, i)) => self.chunks[k].as_ref().map_or(0, |ch| ch.mat.get(i)),
            None => 0,
        }
    }

    /// Occupe (ou libère) une cellule.
    pub fn set(&mut self, c: IVec3, solid: bool, material: u8) {
        let Some((k, i)) = self.locate(c) else { return };
        let bit = 1u64 << (i & 63);
        if solid {
            let ch = self.chunks[k].get_or_insert_with(Chunk::new);
            if ch.solid[i >> 6] & bit == 0 {
                ch.solid[i >> 6] |= bit;
                self.alive += 1;
            }
            ch.mat.set(i, material);
        } else {
            let Some(ch) = self.chunks[k].as_mut() else { return };
            if ch.solid[i >> 6] & bit == 0 {
                return;
            }
            ch.solid[i >> 6] &= !bit;
            self.alive = self.alive.saturating_sub(1);
        }
        self.touch(c);
    }

    /// Crée une porte fermée : `closed[i]` devient `open[i]` à l'ouverture.
    /// `material(i)` donne le matériau du voxel `i`.
    pub fn add_door(&mut self, closed: Vec<IVec3>, open: Vec<IVec3>, material: impl Fn(usize) -> u8) {
        debug_assert_eq!(closed.len(), open.len());
        for (i, &c) in closed.iter().enumerate() {
            self.set(c, true, material(i));
        }
        let total = closed.len();
        self.doors.push(Door {
            closed,
            open,
            is_open: false,
            total,
        });
    }

    /// Ouvre ou ferme une porte. Renvoie les modifications faites
    /// (`[x, y, z, matériau]`, 255 = voxel retiré), ou `None` si quelque
    /// chose bloque le passage.
    pub fn toggle_door(&mut self, index: usize) -> Option<Vec<[u16; 4]>> {
        let door = self.doors.get(index)?;
        let (from, to) = if door.is_open {
            (door.open.clone(), door.closed.clone())
        } else {
            (door.closed.clone(), door.open.clone())
        };
        // Cassée : trop de voxels manquent, elle ne se manœuvre plus.
        let present = from.iter().filter(|&&c| self.is_solid(c)).count();
        if present * 4 < door.total * 3 {
            return None;
        }
        if from
            .iter()
            .zip(&to)
            .any(|(&f, &t)| self.is_solid(f) && self.is_solid(t))
        {
            return None;
        }
        let mut edits = Vec::new();
        for (&f, &t) in from.iter().zip(&to) {
            if !self.is_solid(f) {
                continue;
            }
            let m = self.material(f);
            self.set(f, false, 0);
            self.set(t, true, m);
            edits.push([f.x as u16, f.y as u16, f.z as u16, 255]);
            edits.push([t.x as u16, t.y as u16, t.z as u16, m as u16]);
        }
        self.doors[index].is_open = !self.doors[index].is_open;
        Some(edits)
    }

    /// Porte la plus proche (et visée) à moins de `reach` mètres.
    pub fn door_at(&self, origin: Vec3, dir: Vec3, reach: f32) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (i, door) in self.doors.iter().enumerate() {
            let c = door.center(self);
            let to = c - origin;
            let along = to.dot(dir);
            if along < 0.0 || along > reach {
                continue;
            }
            // Distance du centre au rayon : la porte fait ~1,6 × 2,4 m.
            let off = (to - dir * along).length();
            if off < 1.3 && best.is_none_or(|(_, d)| along < d) {
                best = Some((i, along));
            }
        }
        best.map(|(i, _)| i)
    }

    /// Intégrité structurelle : après une destruction, retire du monde les
    /// voxels qui ne tiennent plus et les renvoie, un `Vec` par ensemble
    /// (ils deviennent des corps rigides, voir `sim`). Le mobilier
    /// (`is_prop`) forme ses propres ensembles.
    ///
    /// Modèle : chaque voxel doit être relié au sol par un chemin dont le
    /// coût reste sous [`STABILITY`]. Monter depuis un appui coûte presque
    /// rien (un pilier porte tout), avancer à l'horizontale coûte selon le
    /// matériau (béton ≈ 5 m de portée, brique ≈ 3,5 m, bois et verre
    /// moins), être suspendu coûte cher. Un plancher sans pilier ni mur
    /// proche, une façade au-dessus d'un grand trou, un porte-à-faux trop
    /// long : tombent. Calcul local, autour de `destroyed`.
    pub fn collapse_components(&mut self, destroyed: &[IVec3]) -> Vec<Vec<IVec3>> {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        if destroyed.is_empty() {
            return Vec::new();
        }
        let ground = crate::map::GROUND;
        let mut lo = destroyed[0];
        let mut hi = destroyed[0];
        for &c in destroyed {
            lo = lo.min(c);
            hi = hi.max(c);
        }
        // Marge : plus que la plus longue portée possible.
        const MARGIN: i32 = 24;
        // Jusqu'au sol : c'est lui qui porte tout.
        let r0 = IVec3::new((lo.x - MARGIN).max(0), (ground - 1).min(lo.y).max(0), (lo.z - MARGIN).max(0));
        let r1 = IVec3::new(
            (hi.x + MARGIN + 1).min(self.dims.x),
            self.dims.y,
            (hi.z + MARGIN + 1).min(self.dims.z),
        );
        let size = r1 - r0;
        if size.x <= 0 || size.y <= 0 || size.z <= 0 {
            return Vec::new();
        }
        let total = (size.x * size.y * size.z) as usize;
        if total > 6_000_000 {
            return Vec::new();
        }
        let index = |c: IVec3| -> usize {
            (((c.x - r0.x) * size.y + (c.y - r0.y)) * size.z + (c.z - r0.z)) as usize
        };
        let inside = |c: IVec3| c.cmpge(r0).all() && c.cmplt(r1).all();

        // Voxels de la région, sources (sol, bords) et matériaux.
        const INF: u32 = u32::MAX;
        let mut dist: Vec<u32> = vec![INF; total];
        let mut solid: Vec<bool> = vec![false; total];
        let mut heap: BinaryHeap<Reverse<(u32, i32, i32, i32)>> = BinaryHeap::new();
        let mut cells: Vec<IVec3> = Vec::new();
        for x in r0.x..r1.x {
            for z in r0.z..r1.z {
                let edge_xz = x == r0.x || x == r1.x - 1 || z == r0.z || z == r1.z - 1;
                for y in r0.y..r1.y {
                    let c = IVec3::new(x, y, z);
                    if !self.is_solid(c) {
                        continue;
                    }
                    let i = index(c);
                    solid[i] = true;
                    cells.push(c);
                    let edge_top = y == r1.y - 1;
                    if y < ground || edge_xz || edge_top {
                        dist[i] = 0;
                        heap.push(Reverse((0, x, y, z)));
                    }
                }
            }
        }
        const NEIGHBORS: [IVec3; 6] = [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z];
        while let Some(Reverse((d, x, y, z))) = heap.pop() {
            let u = IVec3::new(x, y, z);
            if d > dist[index(u)] {
                continue;
            }
            let u_prop = is_prop(self.material(u));
            for o in NEIGHBORS {
                let n = u + o;
                if !inside(n) {
                    continue;
                }
                let ni = index(n);
                if !solid[ni] {
                    continue;
                }
                let m = self.material(n);
                // Une structure ne s'appuie pas sur du mobilier.
                if u_prop && !is_prop(m) {
                    continue;
                }
                let step = if o.y > 0 {
                    // n est au-dessus de u : posé dessus.
                    1
                } else if o.y < 0 {
                    // n est sous u : suspendu à u.
                    28
                } else {
                    span_cost(m) * 10
                };
                // Les coûts sont en dixièmes : 1 = 0,1 par voxel porté.
                let nd = d + step;
                if nd < STABILITY * 10 && nd < dist[ni] {
                    dist[ni] = nd;
                    heap.push(Reverse((nd, n.x, n.y, n.z)));
                }
            }
        }

        // Instables : pleins mais jamais atteints.
        let mut unstable: Vec<IVec3> = Vec::new();
        for &c in &cells {
            if dist[index(c)] == INF {
                unstable.push(c);
            }
        }
        if unstable.is_empty() {
            return Vec::new();
        }
        let mut in_set: HashSet<IVec3> = unstable.iter().copied().collect();
        // Ensembles connexes de même classe (structure / mobilier).
        let mut out: Vec<Vec<IVec3>> = Vec::new();
        for &seed in &unstable {
            if !in_set.remove(&seed) {
                continue;
            }
            let class = is_prop(self.material(seed));
            let mut comp = vec![seed];
            let mut stack = vec![seed];
            while let Some(cur) = stack.pop() {
                for o in NEIGHBORS {
                    let n = cur + o;
                    if in_set.contains(&n) && is_prop(self.material(n)) == class {
                        in_set.remove(&n);
                        comp.push(n);
                        stack.push(n);
                    }
                }
            }
            out.push(comp);
        }
        for comp in &out {
            for &c in comp {
                self.set(c, false, 0);
            }
        }
        out
    }

    /// Variante à plat : toutes les cellules retirées.
    pub fn collapse(&mut self, destroyed: &[IVec3]) -> Vec<IVec3> {
        self.collapse_components(destroyed).into_iter().flatten().collect()
    }

    /// Nombre de morceaux par axe.
    pub fn chunk_grid(&self) -> IVec3 {
        self.chunk_dims
    }

    /// Le morceau contient-il (ou a-t-il contenu) du solide ?
    #[inline]
    pub fn chunk_allocated(&self, k: IVec3) -> bool {
        let d = self.chunk_dims;
        if k.x < 0 || k.y < 0 || k.z < 0 || k.x >= d.x || k.y >= d.y || k.z >= d.z {
            return false;
        }
        self.chunks[((k.x * d.y + k.y) * d.z + k.z) as usize].is_some()
    }

    /// Clefs des morceaux qui contiennent (ou ont contenu) du solide.
    pub fn allocated_chunks(&self) -> Vec<IVec3> {
        let cd = self.chunk_dims;
        let mut out = Vec::new();
        for (n, ch) in self.chunks.iter().enumerate() {
            if ch.is_some() {
                let n = n as i32;
                out.push(IVec3::new(n / (cd.y * cd.z), (n / cd.z) % cd.y, n % cd.z));
            }
        }
        out
    }

    /// Vide toute la grille.
    pub fn clear(&mut self) {
        self.chunks.iter_mut().for_each(|c| *c = None);
        self.doors.clear();
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
    /// Lampes : maillage séparé, lumineux et sans ombre.
    pub lamp_material: Handle<StandardMaterial>,
    /// Traînée de balle.
    pub tracer_cube: Handle<Mesh>,
    pub tracer_material: Handle<StandardMaterial>,
}

/// Palette (sRGB) : couleur par identifiant de matériau.
pub const PALETTE: [(f32, f32, f32); 22] = [
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
    (1.00, 0.93, 0.62), // 16 lampe (émissive)
    (0.62, 0.12, 0.10), // 17 rouge (bouches d'incendie, sièges)
    (0.22, 0.32, 0.58), // 18 bleu (canapés, bennes)
    (0.07, 0.07, 0.08), // 19 noir (pneus, écrans)
    (0.72, 0.52, 0.30), // 20 bois de mobilier
    (0.34, 0.36, 0.40), // 21 métal de mobilier
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
    /// Émissive : dessinée dans un maillage à part, sans ombrage.
    pub const LAMP: u8 = 16;
    pub const RED: u8 = 17;
    pub const BLUE: u8 = 18;
    pub const BLACK: u8 = 19;
    /// Mobilier : n'appartient pas à la structure d'un bâtiment.
    pub const FURN_WOOD: u8 = 20;
    pub const FURN_METAL: u8 = 21;

    /// Matériaux proposés par l'outil de construction.
    pub const BUILD: [u8; 9] = [WOOD, BRICK, CONCRETE, METAL, GLASS, PLASTER, ROOF, BLACK, LAMP];
}

/// Les « éléments » (mobilier, lampes, bouches d'incendie…) ne font pas
/// partie de la structure : ils reposent sur elle et tombent seuls.
pub fn is_prop(material: u8) -> bool {
    use mat::*;
    matches!(material, BLUE | RED | BLACK | LAMP | FURN_WOOD | FURN_METAL)
}

/// Le matériau brûle-t-il ? (bois, feuillage, tissu)
pub fn flammable(material: u8) -> bool {
    use mat::*;
    matches!(material, WOOD | FURN_WOOD | LEAVES | BLUE)
}

/// Durée de combustion d'un voxel (secondes).
pub fn burn_time(material: u8) -> f32 {
    use mat::*;
    match material {
        LEAVES => 3.5,
        BLUE => 4.5,
        FURN_WOOD => 6.0,
        _ => 8.0,
    }
}

/// Résistance horizontale d'un matériau : « coût » d'un voxel porté en
/// porte-à-faux (plus il est faible, plus la portée sans appui est longue).
/// La structure tient tant que chaque voxel est relié au sol par un chemin
/// de coût total inférieur à [`STABILITY`].
fn span_cost(material: u8) -> u32 {
    use mat::*;
    match material {
        METAL | FURN_METAL => 3,
        CONCRETE | CONCRETE_DARK => 4,
        ROOF => 5,
        BRICK | BRICK_DARK => 5,
        MORTAR | PLASTER => 5,
        WOOD | FURN_WOOD => 4,
        LEAVES => 9,
        GLASS => 12,
        BLUE | RED | BLACK | LAMP => 10,
        _ => 1,
    }
}

/// Budget de stabilité d'un voxel (en dixièmes de coût).
pub const STABILITY: u32 = 48;

/// Masse volumique relative d'un matériau (sert au poids des gravats).
pub fn density(material: u8) -> f32 {
    use mat::*;
    match material {
        METAL => 3.0,
        CONCRETE | CONCRETE_DARK | MORTAR | PLASTER => 2.2,
        BRICK | BRICK_DARK | ROOF => 2.0,
        GLASS => 1.6,
        DIRT | ASPHALT => 1.8,
        WOOD | LEAVES | RED | BLUE | FURN_WOOD => 0.7,
        FURN_METAL => 2.5,
        _ => 1.0,
    }
}

/// Nom d'un matériau (HUD de l'outil de construction).
pub fn material_name(material: u8) -> &'static str {
    use mat::*;
    match material {
        WOOD => "Bois",
        BRICK => "Brique",
        CONCRETE => "Béton",
        METAL => "Métal",
        GLASS => "Verre",
        PLASTER => "Plâtre",
        ROOF => "Tuile",
        BLACK => "Noir",
        LAMP => "Lumière",
        _ => "Bloc",
    }
}

/// Argent rapporté par un voxel détruit (mode normal). Le sol ne rapporte
/// rien : sinon il suffirait de creuser pour devenir riche.
pub fn material_value(material: u8) -> u32 {
    use mat::*;
    match material {
        BRICK | BRICK_DARK | GLASS | ROOF => 2,
        LAMP => 3,
        METAL | FURN_METAL => 3,
        CONCRETE | CONCRETE_DARK | MORTAR | WOOD | PLASTER | FURN_WOOD => 1,
        _ => 0,
    }
}

/// Texture d'un voxel (32 × 32, avec mipmaps) : grain, léger biseau et joint
/// sombre. Les UV du maillage valent 1 par voxel : chaque voxel reçoit son
/// propre grain, même sur un grand rectangle fusionné.
fn voxel_texture() -> Image {
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    const SIZE: usize = 32;
    let mut level: Vec<f32> = vec![0.0; SIZE * SIZE];
    let mut seed: u32 = 0x9E37_79B9;
    for y in 0..SIZE {
        for x in 0..SIZE {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = ((seed >> 16) & 0xFF) as f32 / 255.0 - 0.5;
            // Grain fin + un peu de plaques plus larges.
            let blob = (((x / 8) * 31 + (y / 8) * 17) % 7) as f32 / 7.0 - 0.5;
            let mut v = 0.9 + noise * 0.10 + blob * 0.05;
            // Joint sombre sur deux bords, biseau clair sur les autres.
            if x == SIZE - 1 || y == SIZE - 1 {
                v *= 0.72;
            } else if x == 0 || y == 0 {
                v *= 1.06;
            }
            level[y * SIZE + x] = v.clamp(0.0, 1.0);
        }
    }
    let mut data: Vec<u8> = Vec::new();
    let mut mips = 0;
    let mut size = SIZE;
    let mut cur = level;
    loop {
        for v in &cur {
            let b = (v * 255.0) as u8;
            data.extend_from_slice(&[b, b, b, 255]);
        }
        mips += 1;
        if size == 1 {
            break;
        }
        let next = size / 2;
        let mut down = vec![0.0f32; next * next];
        for y in 0..next {
            for x in 0..next {
                let s = cur[(2 * y) * size + 2 * x]
                    + cur[(2 * y) * size + 2 * x + 1]
                    + cur[(2 * y + 1) * size + 2 * x]
                    + cur[(2 * y + 1) * size + 2 * x + 1];
                down[y * next + x] = s * 0.25;
            }
        }
        cur = down;
        size = next;
    }
    let base_len = SIZE * SIZE * 4;
    let mut image = Image::new(
        Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data[..base_len].to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = mips;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    image
}

/// Crée les meshes et la palette.
pub fn setup_voxel_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
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
            base_color_texture: Some(images.add(voxel_texture())),
            // Les UV du maillage sont divisés par `UV_SCALE`.
            uv_transform: bevy::math::Affine2::from_scale(Vec2::splat(crate::vmesh::UV_SCALE)),
            perceptual_roughness: 0.9,
            ..default()
        }),
        lamp_material: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            emissive: LinearRgba::rgb(2.4, 2.0, 1.2),
            unlit: true,
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
    // Décor des menus : une petite ville (la vraie est construite à l'entrée
    // en partie). Économise ~150 Mo de mémoire.
    *world = VoxelWorld::with_blocks(7);
    world.rebuild(game.seed);
}

/// `F5` reconstruit la même ville, `F6` en tire une nouvelle (solo).
pub fn handle_reset(
    keys: Res<ButtonInput<KeyCode>>,
    mut world: ResMut<VoxelWorld>,
    mut game: ResMut<crate::game::GameState>,
    mut net: ResMut<crate::net::client::NetClient>,
    lobby: Res<crate::net::client::Lobby>,
) {
    // En ligne, la carte appartient au serveur : en solo (serveur privé),
    // F5 / F6 le lui demandent.
    if net.is_online() {
        if lobby.solo && game.in_game() {
            let command = if keys.just_pressed(KeyCode::F5) {
                Some(format!("reset {}", game.seed))
            } else if keys.just_pressed(KeyCode::F6) {
                Some("reset".to_string())
            } else {
                None
            };
            if let Some(text) = command {
                net.send_raw(crate::net::protocol::Packet::Command { text });
            }
        }
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

/// Destructeur commun côté client (masse, balle, explosion) : creuse la
/// sphère pour un retour immédiat et lance quelques débris décoratifs. Les
/// gravats, l'effondrement et l'argent viennent du serveur.
#[allow(clippy::too_many_arguments)]
pub fn damage(
    world: &mut VoxelWorld,
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
    fn floating_part_collapses_but_attached_part_stays() {
        let mut w = VoxelWorld::new();
        let g = crate::map::GROUND;
        // Sol, un pilier et une dalle posée dessus.
        w.fill(IVec3::new(0, 1, 0), IVec3::new(40, g, 40), mat::DIRT);
        w.fill(IVec3::new(10, g, 10), IVec3::new(12, g + 10, 12), mat::CONCRETE);
        w.fill(IVec3::new(10, g + 10, 10), IVec3::new(20, g + 12, 20), mat::CONCRETE);
        // On coupe le pilier : la dalle ne tient plus.
        let cut = w.carve(w.cell_center(IVec3::new(10, g + 4, 10)), 1.5);
        assert!(!cut.is_empty());
        let fallen = w.collapse(&cut);
        assert!(fallen.iter().any(|c| c.y >= g + 10), "la dalle tombe");
        assert!(fallen.iter().all(|&c| !w.is_solid(c)));
        // Ce qui est relié au sol reste.
        assert!(w.is_solid(IVec3::new(5, g - 1, 5)));
    }

    #[test]
    fn door_toggles_and_round_trips() {
        let mut w = VoxelWorld::new();
        let closed: Vec<IVec3> = (0..4).map(|i| IVec3::new(5 + i, 10, 5)).collect();
        let open: Vec<IVec3> = (0..4).map(|i| IVec3::new(5, 10, 6 + i)).collect();
        w.add_door(closed.clone(), open.clone(), |_| mat::WOOD);
        assert!(closed.iter().all(|&c| w.is_solid(c)));
        let edits = w.toggle_door(0).expect("s'ouvre");
        assert_eq!(edits.len(), 8);
        assert!(open.iter().all(|&c| w.is_solid(c)));
        assert!(closed.iter().all(|&c| !w.is_solid(c)));
        // Bloquée : un voxel sur le chemin du retour.
        w.set(closed[1], true, mat::BRICK);
        assert!(w.toggle_door(0).is_none());
        w.set(closed[1], false, 0);
        assert!(w.toggle_door(0).is_some());
        assert!(closed.iter().all(|&c| w.is_solid(c)));
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

#[cfg(test)]
mod structure_tests {
    use super::*;

    /// Une ville intacte doit être stable : aucun voxel « instable » dans
    /// des zones prises au hasard (sinon les bâtiments s'écrouleraient au
    /// premier coup).
    #[test]
    fn untouched_city_is_stable() {
        let mut w = VoxelWorld::new();
        w.rebuild(31415);
        let g = crate::map::GROUND;
        let mut checked = 0;
        for (bx, bz) in [(3, 3), (7, 5), (12, 9), (16, 16), (20, 11), (25, 27), (8, 22), (14, 4)] {
            // Centre d'un pâté de maisons.
            let c = IVec3::new(bx * 64 + 32, g + 8, bz * 64 + 32);
            let found = w.collapse_components(&[c]);
            let n: usize = found.iter().map(|v| v.len()).sum();
            assert!(n <= 200, "pâté ({bx},{bz}) : {n} voxels instables dans une ville intacte");
            checked += 1;
        }
        assert_eq!(checked, 8);
    }

    #[test]
    fn wide_hole_makes_the_facade_above_fall_but_small_hole_does_not() {
        let mut w = VoxelWorld::new();
        let g = crate::map::GROUND;
        // Sol + un mur de béton de 60 × 20 voxels, 2 d'épaisseur.
        w.fill(IVec3::new(0, 1, 0), IVec3::new(200, g, 200), mat::DIRT);
        w.fill(IVec3::new(50, g, 50), IVec3::new(110, g + 20, 52), mat::CONCRETE);
        // Petit trou (6 voxels de large) : le linteau tient.
        let mut small = Vec::new();
        for x in 78..84 {
            for y in g..g + 6 {
                for z in 50..52 {
                    w.set(IVec3::new(x, y, z), false, 0);
                    small.push(IVec3::new(x, y, z));
                }
            }
        }
        assert!(w.collapse_components(&small).is_empty());
        // Grand trou (40 voxels) : le mur au-dessus tombe.
        let mut big = Vec::new();
        for x in 60..100 {
            for y in g..g + 6 {
                for z in 50..52 {
                    if w.is_solid(IVec3::new(x, y, z)) {
                        w.set(IVec3::new(x, y, z), false, 0);
                        big.push(IVec3::new(x, y, z));
                    }
                }
            }
        }
        let fallen: usize = w.collapse_components(&big).iter().map(|v| v.len()).sum();
        assert!(fallen > 100, "{fallen} voxels tombés");
    }
}

#[cfg(test)]
mod structure_probe {
    use super::*;

    #[test]
    #[ignore]
    fn probe_unstable() {
        let mut w = VoxelWorld::new();
        w.rebuild(31415);
        let g = crate::map::GROUND;
        for (bx, bz) in [(3, 3), (7, 5), (12, 9), (16, 16), (8, 22), (25, 27)] {
            let c = IVec3::new(bx * 64 + 32, g + 8, bz * 64 + 32);
            let found = w.collapse_components(&[c]);
            let mut by_mat = std::collections::BTreeMap::new();
            let mut lo = IVec3::splat(i32::MAX);
            let mut hi = IVec3::splat(i32::MIN);
            for comp in &found {
                for &cell in comp {
                    *by_mat.entry(w.material(cell)).or_insert(0) += 1;
                    lo = lo.min(cell);
                    hi = hi.max(cell);
                }
            }
            println!("pâté ({bx},{bz}) : {} ensembles, matériaux {by_mat:?}, boîte {lo:?}..{hi:?}", found.len());
            let mut by_y = std::collections::BTreeMap::new();
            for comp in &found {
                for &cell in comp {
                    *by_y.entry(cell.y).or_insert(0) += 1;
                }
            }
            println!("  par y : {by_y:?}");
            if let Some(comp) = found.first() {
                let c0 = comp[0];
                let col: Vec<String> = (c0.y - 4..c0.y + 3)
                    .map(|y| format!("y{}={}", y, if w.is_solid(IVec3::new(c0.x, y, c0.z)) { w.material(IVec3::new(c0.x, y, c0.z)) as i32 } else { -1 }))
                    .collect();
                println!("  colonne sous l'exemple : {col:?}");
                let sample: Vec<_> = comp.iter().take(6).collect();
                println!("  exemple : {sample:?}");
            }
        }
    }
}
