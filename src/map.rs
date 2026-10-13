//! Génération procédurale de la ville à partir d'une **seed**.
//!
//! La carte est un quadrillage de rues ; chaque pâté de maisons reçoit, au
//! hasard (mais de façon reproductible) : une tour de bureaux, un immeuble
//! de briques, un entrepôt, une tour de verre, des maisons, un parc, un
//! parking ou une zone de chantier. Tout reste destructible, sauf la couche
//! de socle tout en bas.

use bevy::prelude::*;

use crate::world::mat::*;
use crate::world::{VoxelWorld, VOXEL};

/// Version du générateur : une sauvegarde d'une autre version est refusée
/// (rejouer son historique sur une autre ville n'aurait aucun sens).
pub const MAP_VERSION: u32 = 4;

/// Espacement des rues, en cellules.
pub const BLOCK: i32 = 64;
/// Demi-largeur de la chaussée.
pub const ROAD: i32 = 6;
/// Largeur des trottoirs.
pub const WALK: i32 = 3;
/// Dessus du sol (première cellule vide).
pub const GROUND: i32 = 30;
/// Épaisseur des murs, en voxels.
const WALL: i32 = 2;

/// Une position de trottoir tirée au sort sur toute la carte.
///
/// Le serveur s'en sert pour faire apparaître les joueurs **côté serveur**
/// : n'importe quel point de trottoir libre de la ville, et non un point
/// fixe autour du carrefour central. Renvoie `None` si la carte est trop
/// petite ou entièrement bâtie.
pub fn random_sidewalk(world: &VoxelWorld, salt: u64) -> Option<Vec3> {
    if world.dims.x < BLOCK * 2 || world.dims.z < BLOCK * 2 {
        return None;
    }
    let lines = (world.dims.x / BLOCK).max(1);
    // Le bord intérieur du trottoir : `c ± (ROAD + 1)` (voir `street_props`).
    let edge = ROAD + 1;
    let mut rng = MapRng::new(world.seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    // Quarantaine d'essais : on veut un trottoir libre, pas un toit ni un
    // carrefour.
    for _ in 0..48 {
        let k = rng.range(0, lines);
        let along = rng.range(6, world.dims.z.saturating_sub(6));
        let side = if rng.chance(0.5) { -1 } else { 1 };
        let across = k * BLOCK + side * edge;
        // Les deux sens de rue : décalage en X (rue le long de Z) puis en Z.
        for cell in [
            IVec3::new(across, GROUND, along),
            IVec3::new(along, GROUND, across),
        ] {
            if cell.x <= 2 || cell.x >= world.dims.x - 2 || cell.z <= 2 || cell.z >= world.dims.z - 2 {
                continue;
            }
            // Le trottoir est surélevé d'un voxel : on veut son dessus, donc
            // la cellule de surface doit être du béton (les carrefours, eux,
            // sont à l'asphalte au niveau du sol).
            if !in_bounds(world, cell) || world.material(cell) != CONCRETE {
                continue;
            }
            let feet = world.cell_min(cell + IVec3::Y) + Vec3::new(VOXEL * 0.5, 0.06, VOXEL * 0.5);
            // Place libre pour toute la hauteur du joueur ? La boîte va des
            // pieds au sommet de la tête : ne pas redescendre sous `feet`,
            // sinon elle inclut le trottoir et tout candidat est refusé.
            let half = Vec3::new(0.3, crate::player::HEIGHT * 0.5, 0.3);
            let center = feet + Vec3::Y * half.y;
            if world.overlaps(center - half, center + half) {
                continue;
            }
            return Some(feet);
        }
    }
    None
}

/// La cellule est-elle dans la grille ?
fn in_bounds(world: &VoxelWorld, c: IVec3) -> bool {
    c.cmpge(IVec3::ZERO).all() && c.cmplt(world.dims).all()
}

/// Générateur pseudo-aléatoire déterministe (splitmix64).
pub struct MapRng(u64);

impl MapRng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x5DEE_CE66_D1CE_4E5B)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Entier dans `[lo, hi]`.
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }

    pub fn chance(&mut self, p: f32) -> bool {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32 <= p
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[(self.next() % items.len() as u64) as usize]
    }
}

/// Construit toute la carte et renvoie le point d'apparition (pieds).
pub fn generate(w: &mut VoxelWorld, seed: u64) -> Vec3 {
    let mut rng = MapRng::new(seed);
    let d = w.dims;

    // --- Sol : socle, terre, herbe ---
    // Sol profond : socle, roche, puis plusieurs mètres de terre (assez
    // pour creuser des cratères et loger des bunkers).
    w.fill(IVec3::ZERO, IVec3::new(d.x, 1, d.z), BEDROCK);
    w.fill(IVec3::new(0, 1, 0), IVec3::new(d.x, GROUND - 12, d.z), STONE);
    w.fill(IVec3::new(0, GROUND - 12, 0), IVec3::new(d.x, GROUND - 1, d.z), DIRT);
    w.fill(IVec3::new(0, GROUND - 1, 0), IVec3::new(d.x, GROUND, d.z), GRASS);

    // --- Rues (les deux sens) ---
    let lines: Vec<i32> = (0..=d.x / BLOCK).map(|k| k * BLOCK).collect();
    for &c in &lines {
        // Chaussée + trottoirs le long de Z…
        road(w, c, true);
        // … et le long de X.
        road(w, c, false);
    }
    // Les carrefours n'ont pas de bordure.
    for &cx in &lines {
        for &cz in &lines {
            w.fill(
                IVec3::new(cx - ROAD, GROUND - 1, cz - ROAD),
                IVec3::new(cx + ROAD, GROUND, cz + ROAD),
                ASPHALT,
            );
            w.empty(
                IVec3::new(cx - ROAD - WALK, GROUND, cz - ROAD - WALK),
                IVec3::new(cx + ROAD + WALK, GROUND + 1, cz + ROAD + WALK),
            );
            w.fill(
                IVec3::new(cx - ROAD - WALK, GROUND - 1, cz - ROAD - WALK),
                IVec3::new(cx + ROAD + WALK, GROUND, cz + ROAD + WALK),
                ASPHALT,
            );
        }
    }

    // --- Pâtés de maisons ---
    let margin = ROAD + WALK + 1;
    for bx in 0..d.x / BLOCK {
        for bz in 0..d.z / BLOCK {
            let min = IVec3::new(bx * BLOCK + margin, GROUND, bz * BLOCK + margin);
            let max = IVec3::new((bx + 1) * BLOCK - margin, GROUND, (bz + 1) * BLOCK - margin);
            // Distance au centre (en pâtés de maisons) : centre-ville dense
            // et haut, banlieue de maisons et d'entrepôts.
            let cx = d.x / BLOCK / 2;
            let cz = d.z / BLOCK / 2;
            // Les districts s'étirent avec la taille de la carte.
            let ring = (bx - cx).abs().max((bz - cz).abs()) * 16 / (d.x / BLOCK / 2).max(1);
            lot(w, &mut rng, min, max, ring);
        }
    }

    street_props(w, &mut rng);

    // --- Mur d'enceinte ---
    let h = GROUND + 8;
    for (a, b) in [
        (IVec3::new(0, GROUND, 0), IVec3::new(2, h, d.z)),
        (IVec3::new(d.x - 2, GROUND, 0), IVec3::new(d.x, h, d.z)),
        (IVec3::new(0, GROUND, 0), IVec3::new(d.x, h, 2)),
        (IVec3::new(0, GROUND, d.z - 2), IVec3::new(d.x, h, d.z)),
    ] {
        w.fill(a, b, CONCRETE_DARK);
    }

    // Apparition : carrefour central (toujours dégagé), un peu décalé.
    let center = IVec3::new((d.x / 2 / BLOCK) * BLOCK, GROUND, (d.z / 2 / BLOCK) * BLOCK);
    let spawn = w.cell_min(center) + Vec3::new(0.2, 0.05, 0.2);
    w.empty(center - IVec3::new(2, 0, 2), center + IVec3::new(3, 8, 3));
    spawn
}

/// Une rue centrée sur `c` (le long de Z si `along_z`, sinon de X).
fn road(w: &mut VoxelWorld, c: i32, along_z: bool) {
    let len = if along_z { w.dims.z } else { w.dims.x };
    let bx = |a: i32, b: i32, y0: i32, y1: i32| -> (IVec3, IVec3) {
        if along_z {
            (IVec3::new(a, y0, 0), IVec3::new(b, y1, len))
        } else {
            (IVec3::new(0, y0, a), IVec3::new(len, y1, b))
        }
    };
    let (a, b) = bx(c - ROAD, c + ROAD, GROUND - 1, GROUND);
    w.fill(a, b, ASPHALT);
    // Trottoirs surélevés d'un voxel (on les monte sans sauter).
    for (s0, s1) in [(c - ROAD - WALK, c - ROAD), (c + ROAD, c + ROAD + WALK)] {
        let (a, b) = bx(s0, s1, GROUND - 1, GROUND + 1);
        w.fill(a, b, CONCRETE);
    }
    // Pointillés au milieu.
    let mut t = 0;
    while t < len {
        let (a, b) = if along_z {
            (IVec3::new(c, GROUND - 1, t), IVec3::new(c + 1, GROUND, t + 4))
        } else {
            (IVec3::new(t, GROUND - 1, c), IVec3::new(t + 4, GROUND, c + 1))
        };
        w.fill(a, b, PAINT);
        t += 10;
    }
}

/// Remplit un pâté de maisons. `ring` : distance au centre-ville, en pâtés.
fn lot(w: &mut VoxelWorld, rng: &mut MapRng, min: IVec3, max: IVec3, ring: i32) {
    let roll = rng.range(0, 99);
    match ring {
        // Centre-ville : tours et bureaux.
        0..=5 => match roll {
            0..=34 => office(w, rng, min, max),
            35..=59 => glass_tower(w, rng, min, max),
            60..=79 => apartment(w, rng, min, max),
            80..=87 => park(w, rng, min, max),
            88..=94 => parking(w, rng, min, max),
            _ => construction(w, rng, min, max),
        },
        // Quartiers mixtes.
        6..=11 => match roll {
            0..=17 => office(w, rng, min, max),
            18..=37 => apartment(w, rng, min, max),
            38..=57 => houses(w, rng, min, max),
            58..=69 => warehouse(w, rng, min, max),
            70..=81 => park(w, rng, min, max),
            82..=91 => parking(w, rng, min, max),
            _ => construction(w, rng, min, max),
        },
        // Périphérie : maisons, entrepôts, parcs.
        _ => match roll {
            0..=49 => houses(w, rng, min, max),
            50..=69 => warehouse(w, rng, min, max),
            70..=84 => park(w, rng, min, max),
            85..=92 => parking(w, rng, min, max),
            93..=96 => apartment(w, rng, min, max),
            _ => construction(w, rng, min, max),
        },
    }
}

/// Style d'un bâtiment.
struct Style {
    wall: u8,
    /// Matériau d'accent (colonnes, bandeaux).
    accent: u8,
    floor: u8,
    roof: u8,
    /// Hauteur d'un étage (dalle comprise).
    storey: i32,
    /// Fenêtres : largeur, espacement.
    window: i32,
    spacing: i32,
    /// Ajoute un escalier entre les étages.
    stairs: bool,
}

/// Un bâtiment : murs de deux voxels, étages, escaliers, fenêtres vitrées,
/// portes sur toutes les faces, toit.
fn building(w: &mut VoxelWorld, rng: &mut MapRng, min: IVec3, max: IVec3, s: &Style) {
    let height = max.y - min.y;
    let mid_x = (min.x + max.x) / 2;
    let mid_z = (min.z + max.z) / 2;

    for y in 0..height {
        let in_storey = y % s.storey;
        let window_row = in_storey >= 3 && in_storey < s.storey - 2;
        for z in min.z..max.z {
            for x in min.x..max.x {
                let edge_x = x < min.x + WALL || x >= max.x - WALL;
                let edge_z = z < min.z + WALL || z >= max.z - WALL;
                if !edge_x && !edge_z {
                    continue;
                }
                let cell = IVec3::new(x, min.y + y, z);
                let outer = x == min.x || x == max.x - 1 || z == min.z || z == max.z - 1;
                let corner = edge_x && edge_z;

                // Portes (rez-de-chaussée, au milieu de chaque face).
                if y < 6 && !corner {
                    let along = if edge_x { z } else { x };
                    let mid = if edge_x { mid_z } else { mid_x };
                    if (mid - 2..mid + 2).contains(&along) {
                        continue;
                    }
                }

                // Fenêtres : vitre côté extérieur, vide côté intérieur.
                if window_row && y >= s.storey && !corner && s.window > 0 {
                    let along = if edge_x { z - min.z } else { x - min.x };
                    let k = along % s.spacing;
                    if k >= (s.spacing - s.window) / 2 && k < (s.spacing - s.window) / 2 + s.window {
                        if outer {
                            w.set(cell, true, GLASS);
                        }
                        continue;
                    }
                }
                if window_row && y < s.storey && y >= 2 && !corner && s.window > 0 {
                    // Vitrines au rez-de-chaussée, plus larges.
                    let along = if edge_x { z - min.z } else { x - min.x };
                    let k = along % s.spacing;
                    if k >= 1 && k < s.spacing - 1 && in_storey < 5 {
                        if outer {
                            w.set(cell, true, GLASS);
                        }
                        continue;
                    }
                }

                // Bandeaux et colonnes pour le relief.
                let m = if corner || in_storey == 0 {
                    s.accent
                } else if s.wall == BRICK && (x + z + y) % 7 == 0 {
                    BRICK_DARK
                } else {
                    s.wall
                };
                w.set(cell, true, m);
            }
        }
    }

    // Dalles d'étage.
    let mut y = min.y + s.storey;
    let mut floors = Vec::new();
    while y < max.y - 3 {
        w.fill(
            IVec3::new(min.x + WALL, y - 1, min.z + WALL),
            IVec3::new(max.x - WALL, y, max.z - WALL),
            s.floor,
        );
        floors.push(y);
        y += s.storey;
    }

    // Piliers porteurs : une trame tous les 12 voxels environ, du sol au
    // toit. Sans eux, les grands planchers ne tiendraient pas (voir
    // `VoxelWorld::collapse_components`).
    let inner_w = max.x - min.x - 2 * WALL;
    let inner_d = max.z - min.z - 2 * WALL;
    if inner_w >= 18 && inner_d >= 18 {
        let nx = (inner_w / 12).max(1);
        let nz = (inner_d / 12).max(1);
        for ix in 1..=nx {
            for iz in 1..=nz {
                let x = min.x + WALL + inner_w * ix / (nx + 1);
                let z = min.z + WALL + inner_d * iz / (nz + 1);
                // Pas dans l'escalier (mur nord) ni sur la porte.
                if s.stairs && z < min.z + WALL + 9 {
                    continue;
                }
                w.fill(
                    IVec3::new(x, min.y, z),
                    IVec3::new(x + 2, max.y - 1, z + 2),
                    CONCRETE,
                );
            }
        }
    }

    // Toit avec un petit rebord.
    w.fill(
        IVec3::new(min.x, max.y - 1, min.z),
        IVec3::new(max.x, max.y, max.z),
        s.roof,
    );
    w.fill(IVec3::new(min.x, max.y, min.z), IVec3::new(max.x, max.y + 1, min.z + 1), s.accent);
    w.fill(IVec3::new(min.x, max.y, max.z - 1), IVec3::new(max.x, max.y + 1, max.z), s.accent);
    w.fill(IVec3::new(min.x, max.y, min.z), IVec3::new(min.x + 1, max.y + 1, max.z), s.accent);
    w.fill(IVec3::new(max.x - 1, max.y, min.z), IVec3::new(max.x, max.y + 1, max.z), s.accent);

    // Escaliers en zigzag : deux couloirs le long du mur nord, un étage sur
    // deux dans chaque sens. Les marches font un voxel : on les gravit sans
    // sauter.
    let inner_x = max.x - min.x - 2 * WALL;
    let run = 2 * s.storey;
    if s.stairs && inner_x >= run + 4 && max.z - min.z - 2 * WALL >= 12 {
        let lanes = [min.z + WALL + 1, min.z + WALL + 4];
        let x0 = min.x + WALL + 2;
        let mut base = min.y;
        let mut level = 0;
        for &top in floors.iter().chain(std::iter::once(&(max.y - 1))) {
            let lane = lanes[level % 2];
            let rise = top - base;
            for step in 0..rise {
                let x = if level % 2 == 0 {
                    x0 + step * 2
                } else {
                    x0 + run - 2 - step * 2
                };
                w.fill(
                    IVec3::new(x, base, lane),
                    IVec3::new(x + 2, base + step + 1, lane + 3),
                    s.floor,
                );
            }
            // Trémie au-dessus du couloir (le toit garde une trappe).
            w.empty(
                IVec3::new(x0, top - 1, lane),
                IVec3::new(x0 + run, top, lane + 3),
            );
            base = top;
            level += 1;
        }
    }

    // Portes battantes dans les ouvertures (une face sur deux environ).
    for face in 0..4 {
        if rng.chance(0.6) {
            add_door(w, min, max, face, mid_x, mid_z);
        }
    }

    // Mobilier et lumières, étage par étage.
    let mut levels = vec![min.y];
    levels.extend(floors.iter().copied());
    for &base in &levels {
        furnish(w, rng, min, max, base, s);
    }

    // Quelques caisses au rez-de-chaussée.
    for _ in 0..rng.range(0, 2) {
        let x = rng.range(min.x + WALL + 2, max.x - WALL - 5);
        let z = rng.range(max.z - WALL - 10, max.z - WALL - 4);
        let at = IVec3::new(x, min.y, z);
        if free(w, at, at + IVec3::new(5, 8, 5)) {
            crate_stack(w, rng, at);
        }
    }
}

/// Aucune cellule pleine dans la boîte `[a, b)`.
fn free(w: &VoxelWorld, a: IVec3, b: IVec3) -> bool {
    for y in a.y..b.y {
        for x in a.x..b.x {
            for z in a.z..b.z {
                if w.is_solid(IVec3::new(x, y, z)) {
                    return false;
                }
            }
        }
    }
    true
}

/// Porte battante dans l'ouverture d'une face (0 ouest, 1 est, 2 nord,
/// 3 sud). Ouverte, elle se range contre le mur, côté intérieur.
fn add_door(w: &mut VoxelWorld, min: IVec3, max: IVec3, face: i32, mid_x: i32, mid_z: i32) {
    let mut closed = Vec::new();
    let mut open = Vec::new();
    let mut handle = Vec::new();
    for y in 0..6 {
        for i in 0..4 {
            for t in 0..WALL {
                let yy = min.y + y;
                let (c, o) = match face {
                    0 => (
                        IVec3::new(min.x + t, yy, mid_z - 2 + i),
                        IVec3::new(min.x + WALL + i, yy, mid_z - 2 + t),
                    ),
                    1 => (
                        IVec3::new(max.x - WALL + t, yy, mid_z - 2 + i),
                        IVec3::new(max.x - WALL - 1 - i, yy, mid_z - 2 + t),
                    ),
                    2 => (
                        IVec3::new(mid_x - 2 + i, yy, min.z + t),
                        IVec3::new(mid_x - 2 + t, yy, min.z + WALL + i),
                    ),
                    _ => (
                        IVec3::new(mid_x - 2 + i, yy, max.z - WALL + t),
                        IVec3::new(mid_x - 2 + t, yy, max.z - WALL - 1 - i),
                    ),
                };
                handle.push(i == 3 && y == 3);
                closed.push(c);
                open.push(o);
            }
        }
    }
    w.add_door(closed, open, |i| if handle[i] { METAL } else { WOOD });
}

/// Table et chaises, bureau, canapé, étagère, lampadaire d'intérieur, et
/// plafonniers : tout ce qui rend un étage habité.
fn furnish(w: &mut VoxelWorld, rng: &mut MapRng, min: IVec3, max: IVec3, base: i32, s: &Style) {
    let x0 = min.x + WALL + 1;
    let x1 = max.x - WALL - 1;
    let mut z0 = min.z + WALL + 1;
    let z1 = max.z - WALL - 1;
    // Les escaliers longent le mur nord.
    if s.stairs {
        z0 += 8;
    }
    if x1 - x0 < 8 || z1 - z0 < 8 {
        return;
    }
    let ceiling = base + s.storey - 2;

    // Plafonniers : une plaque lumineuse tous les 8 voxels.
    let mut z = z0 + 2;
    while z < z1 - 2 {
        let mut x = x0 + 2;
        while x < x1 - 2 {
            let at = IVec3::new(x, ceiling, z);
            // Accroché à un vrai plafond (sinon il flotte au dernier étage
            // ou sous un toit plus bas).
            let hung = (0..2).all(|dx| (0..2).all(|dz| w.is_solid(at + IVec3::new(dx, 1, dz))));
            if hung && free(w, at, at + IVec3::new(2, 1, 2)) {
                w.fill(at, at + IVec3::new(2, 1, 2), LAMP);
            }
            x += 8;
        }
        z += 8;
    }

    let area = (x1 - x0) * (z1 - z0);
    let count = (area / 90).clamp(2, 14);
    for _ in 0..count {
        let x = rng.range(x0, x1 - 6);
        let z = rng.range(z0, z1 - 6);
        let at = IVec3::new(x, base, z);
        // Le mobilier repose sur un sol plein (pas au-dessus d'une trémie).
        let floor = |dx: i32, dz: i32| w.is_solid(at + IVec3::new(dx, -1, dz));
        if !(floor(0, 0) && floor(5, 0) && floor(0, 5) && floor(5, 5) && floor(2, 2)) {
            continue;
        }
        match rng.range(0, 5) {
            // Table 4×2 avec quatre chaises.
            0 | 1 => {
                if !free(w, at, at + IVec3::new(6, 4, 6)) {
                    continue;
                }
                let top = at + IVec3::new(1, 0, 2);
                for (dx, dz) in [(0, 0), (3, 0), (0, 1), (3, 1)] {
                    w.set(top + IVec3::new(dx, 0, dz), true, FURN_WOOD);
                }
                w.fill(top + IVec3::new(0, 1, 0), top + IVec3::new(4, 2, 2), FURN_WOOD);
                // Chaises : assise, dossier à l'opposé de la table.
                for (dx, dz, back) in [(1, -1, -1), (2, -1, -1), (1, 2, 1), (2, 2, 1)] {
                    let c = top + IVec3::new(dx, 0, dz);
                    w.set(c, true, BLUE);
                    for h in 0..3 {
                        w.set(c + IVec3::new(0, h, back), true, FURN_WOOD);
                    }
                }
            }
            // Bureau avec écran et chaise.
            2 => {
                if !free(w, at, at + IVec3::new(6, 5, 5)) {
                    continue;
                }
                w.fill(at + IVec3::new(0, 0, 0), at + IVec3::new(1, 1, 2), FURN_METAL);
                w.fill(at + IVec3::new(4, 0, 0), at + IVec3::new(5, 1, 2), FURN_METAL);
                w.fill(at + IVec3::new(0, 1, 0), at + IVec3::new(5, 2, 2), FURN_WOOD);
                w.fill(at + IVec3::new(2, 2, 0), at + IVec3::new(4, 4, 1), BLACK);
                w.set(at + IVec3::new(2, 0, 3), true, BLUE);
                for h in 0..3 {
                    w.set(at + IVec3::new(2, h, 4), true, BLUE);
                }
            }
            // Canapé.
            3 => {
                if !free(w, at, at + IVec3::new(5, 3, 3)) {
                    continue;
                }
                w.fill(at, at + IVec3::new(5, 1, 2), BLUE);
                w.fill(at + IVec3::new(0, 1, 0), at + IVec3::new(5, 2, 1), BLUE);
                w.fill(at + IVec3::new(0, 1, 0), at + IVec3::new(1, 2, 2), BLUE);
                w.fill(at + IVec3::new(4, 1, 0), at + IVec3::new(5, 2, 2), BLUE);
            }
            // Étagère garnie.
            4 => {
                if !free(w, at, at + IVec3::new(6, 6, 2)) {
                    continue;
                }
                w.fill(at, at + IVec3::new(5, 5, 1), FURN_WOOD);
                for shelf in [1, 3] {
                    w.empty(at + IVec3::new(1, shelf, 0), at + IVec3::new(4, shelf + 1, 1));
                    if rng.chance(0.7) {
                        w.set(at + IVec3::new(rng.range(1, 3), shelf, 0), true, RED);
                    }
                }
            }
            // Lampadaire d'intérieur.
            _ => {
                if !free(w, at, at + IVec3::new(1, 6, 1)) {
                    continue;
                }
                w.fill(at, at + IVec3::new(1, 4, 1), FURN_METAL);
                w.fill(at + IVec3::new(0, 4, 0), at + IVec3::new(1, 6, 1), LAMP);
            }
        }
    }
}

/// Mobilier urbain : lampadaires, bancs, bouches d'incendie, poubelles,
/// feux tricolores et arbres d'alignement le long des rues.
fn street_props(w: &mut VoxelWorld, rng: &mut MapRng) {
    let y0 = GROUND + 1;
    let blocks = w.dims.x / BLOCK;
    let len = w.dims.x;
    for k in 0..=blocks {
        let c = k * BLOCK;
        let mut t = 10;
        while t < len - 10 {
            // Pas de mobilier sur les carrefours.
            let m = t % BLOCK;
            let off = m.min(BLOCK - m);
            if off > ROAD + WALK + 3 {
                for side in [-1, 1] {
                    for along_z in [true, false] {
                        // Trottoir : bord intérieur, côté `side`.
                        let a = c + side * (ROAD + 1);
                        let (x, z, toward) = if along_z {
                            (a, t, IVec3::new(-side, 0, 0))
                        } else {
                            (t, a, IVec3::new(0, 0, -side))
                        };
                        let at = IVec3::new(x, y0, z);
                        let pick = (t / 10 + k + side + along_z as i32) % 4;
                        match pick {
                            0 => lamp_post(w, at, toward),
                            1 if rng.chance(0.5) => bench(w, at, along_z),
                            2 if rng.chance(0.5) => {
                                w.fill(at, at + IVec3::new(1, 2, 1), RED);
                                w.set(at + IVec3::new(0, 2, 0), true, FURN_METAL);
                            }
                            3 if rng.chance(0.4) => {
                                w.fill(at, at + IVec3::new(2, 2, 2), BLACK);
                            }
                            _ => {}
                        }
                    }
                }
            }
            t += 10;
        }
        // Feux tricolores aux carrefours.
        for j in 0..=blocks {
            let cz = j * BLOCK;
            let (cx, cz) = (c, cz);
            for (sx, sz) in [(-1, -1), (1, 1)] {
                let mut p = IVec3::new(cx + sx * (ROAD + 1), y0, cz + sz * (ROAD + 1));
                // Posé sur ce qu'il y a vraiment : à l'angle d'un carrefour,
                // il n'y a pas de trottoir mais l'asphalte, plus bas.
                while p.y > 1 && !w.is_solid(p - IVec3::Y) {
                    p.y -= 1;
                }
                w.fill(p, p + IVec3::new(1, 11, 1), FURN_METAL);
                w.fill(p + IVec3::new(0, 8, 0), p + IVec3::new(1, 11, 1), BLACK);
                w.set(p + IVec3::new(0, 9, 0), true, LAMP);
            }
        }
    }
}

/// Lampadaire : mât métallique, bras courbé au-dessus de la chaussée.
fn lamp_post(w: &mut VoxelWorld, at: IVec3, toward: IVec3) {
    if !free(w, at, at + IVec3::new(1, 13, 1)) {
        return;
    }
    w.fill(at, at + IVec3::new(1, 12, 1), FURN_METAL);
    for i in 1..=3 {
        w.set(at + IVec3::new(0, 11, 0) + toward * i, true, FURN_METAL);
    }
    w.set(at + IVec3::new(0, 10, 0) + toward * 3, true, LAMP);
    w.set(at + IVec3::new(0, 10, 0) + toward * 2, true, LAMP);
}

/// Banc : deux pieds et une assise de 4 voxels.
fn bench(w: &mut VoxelWorld, at: IVec3, along_z: bool) {
    let axis = if along_z { IVec3::new(0, 0, 1) } else { IVec3::new(1, 0, 0) };
    if !free(w, at, at + axis * 4 + IVec3::new(1, 3, 1)) {
        return;
    }
    w.set(at, true, FURN_METAL);
    w.set(at + axis * 3, true, FURN_METAL);
    for i in 0..4 {
        w.set(at + axis * i + IVec3::new(0, 1, 0), true, FURN_WOOD);
    }
}

fn office(w: &mut VoxelWorld, rng: &mut MapRng, min: IVec3, max: IVec3) {
    let inset = rng.range(0, 4);
    let storeys = rng.range(3, 5);
    let s = Style {
        wall: rng.pick(&[CONCRETE, PLASTER, CONCRETE]),
        accent: CONCRETE_DARK,
        floor: CONCRETE_DARK,
        roof: CONCRETE_DARK,
        storey: 10,
        window: 4,
        spacing: 6,
        stairs: true,
    };
    let top = (min.y + storeys * s.storey).min(w.dims.y - 3);
    building(
        w,
        rng,
        IVec3::new(min.x + inset, min.y, min.z + inset),
        IVec3::new(max.x - inset, top, max.z - inset),
        &s,
    );
}

fn apartment(w: &mut VoxelWorld, rng: &mut MapRng, min: IVec3, max: IVec3) {
    let storeys = rng.range(2, 4);
    let s = Style {
        wall: rng.pick(&[BRICK, BRICK, BRICK_DARK]),
        accent: MORTAR,
        floor: WOOD,
        roof: rng.pick(&[ROOF, CONCRETE_DARK]),
        storey: 9,
        window: 3,
        spacing: 7,
        stairs: true,
    };
    let split = rng.chance(0.5);
    if split {
        let mid = (min.x + max.x) / 2;
        let top_a = min.y + storeys * s.storey;
        let top_b = min.y + rng.range(2, 4) * s.storey;
        building(w, rng, IVec3::new(min.x, min.y, min.z + 2), IVec3::new(mid - 1, top_a, max.z - 2), &s);
        building(w, rng, IVec3::new(mid + 1, min.y, min.z + 2), IVec3::new(max.x, top_b, max.z - 2), &s);
    } else {
        let top = min.y + storeys * s.storey;
        building(w, rng, IVec3::new(min.x + 2, min.y, min.z + 2), IVec3::new(max.x - 2, top, max.z - 2), &s);
    }
}

fn warehouse(w: &mut VoxelWorld, rng: &mut MapRng, min: IVec3, max: IVec3) {
    let s = Style {
        wall: rng.pick(&[METAL, METAL, ROOF]),
        accent: CONCRETE_DARK,
        floor: CONCRETE,
        roof: METAL,
        storey: 16,
        window: 0,
        spacing: 8,
        stairs: false,
    };
    let bmin = IVec3::new(min.x + 1, min.y, min.z + 6);
    let bmax = IVec3::new(max.x - 1, min.y + rng.range(14, 18), max.z - 1);
    building(w, rng, bmin, bmax, &s);
    // Grande porte de quai.
    let mid = (bmin.x + bmax.x) / 2;
    w.empty(IVec3::new(mid - 5, bmin.y, bmin.z), IVec3::new(mid + 5, bmin.y + 9, bmin.z + WALL));
    // Palettes et caisses devant et dedans.
    for _ in 0..rng.range(3, 7) {
        let x = rng.range(bmin.x + 3, bmax.x - 7);
        let z = rng.range(bmin.z + 4, bmax.z - 7);
        crate_stack(w, rng, IVec3::new(x, min.y, z));
    }
    container(w, rng, IVec3::new(min.x + 2, min.y, min.z));
}

fn glass_tower(w: &mut VoxelWorld, rng: &mut MapRng, min: IVec3, max: IVec3) {
    let inset = rng.range(2, 6);
    let s = Style {
        wall: GLASS,
        accent: METAL,
        floor: CONCRETE,
        roof: METAL,
        storey: 8,
        window: 0,
        spacing: 8,
        stairs: true,
    };
    let top = (min.y + rng.range(5, 6) * s.storey).min(w.dims.y - 3);
    let bmin = IVec3::new(min.x + inset, min.y, min.z + inset);
    let bmax = IVec3::new(max.x - inset, top, max.z - inset);
    building(w, rng, bmin, bmax, &s);
    // Poteaux de structure tous les 6 voxels.
    let mut x = bmin.x;
    while x < bmax.x {
        w.fill(IVec3::new(x, bmin.y, bmin.z), IVec3::new(x + 1, top, bmin.z + 1), METAL);
        w.fill(IVec3::new(x, bmin.y, bmax.z - 1), IVec3::new(x + 1, top, bmax.z), METAL);
        x += 6;
    }
}

fn houses(w: &mut VoxelWorld, rng: &mut MapRng, min: IVec3, max: IVec3) {
    // Quatre parcelles avec pelouse.
    let mx = (min.x + max.x) / 2;
    let mz = (min.z + max.z) / 2;
    for (a, b) in [
        (IVec3::new(min.x, min.y, min.z), IVec3::new(mx, min.y, mz)),
        (IVec3::new(mx, min.y, min.z), IVec3::new(max.x, min.y, mz)),
        (IVec3::new(min.x, min.y, mz), IVec3::new(mx, min.y, max.z)),
        (IVec3::new(mx, min.y, mz), IVec3::new(max.x, min.y, max.z)),
    ] {
        if rng.chance(0.2) {
            tree(w, rng, IVec3::new((a.x + b.x) / 2, min.y, (a.z + b.z) / 2));
            continue;
        }
        let s = Style {
            wall: rng.pick(&[PLASTER, BRICK, WOOD, PLASTER]),
            accent: rng.pick(&[WOOD, MORTAR, CONCRETE_DARK]),
            floor: WOOD,
            roof: ROOF,
            storey: 9,
            window: 3,
            spacing: 6,
            stairs: false,
        };
        let storeys = rng.range(1, 2);
        building(
            w,
            rng,
            IVec3::new(a.x + 2, min.y, a.z + 2),
            IVec3::new(b.x - 2, min.y + storeys * s.storey, b.z - 2),
            &s,
        );
        // Toit à deux pans.
        let (x0, x1) = (a.x + 1, b.x - 1);
        let base = min.y + storeys * s.storey;
        let mut k = 0;
        while x0 + k < x1 - k {
            w.fill(IVec3::new(x0 + k, base + k, a.z + 1), IVec3::new(x1 - k, base + k + 1, b.z - 1), ROOF);
            k += 1;
            if base + k >= w.dims.y - 2 {
                break;
            }
        }
    }
}

/// Bunker enterré : une salle en béton sous la parcelle, éclairée et
/// meublée, reliée à la surface par un escalier bordé de murets. Seulement
/// si l'entrée tombe sur un terrain dégagé.
fn bunker(w: &mut VoxelWorld, rng: &mut MapRng, min: IVec3, max: IVec3) {
    let g = GROUND;
    let (hw, hd) = (rng.range(6, 9), rng.range(4, 6));
    let cx = (min.x + max.x) / 2 - 6;
    // Décalé du centre (fontaine, allées en croix des parcs).
    let cz = (min.z + max.z) / 2 + 9 + rng.range(0, 3);
    let floor = g - 11; // dessus de la dalle du sol
    let top = floor + 6; // première cellule du plafond
    let stairs = top - floor + 4;
    // L'escalier sort côté +X : il doit rester dans la parcelle et à l'air libre.
    let exit_x = cx + hw + 2 + stairs;
    if cx - hw - 1 <= min.x || exit_x + 2 >= max.x || cz - hd - 1 <= min.z || cz + hd + 1 >= max.z {
        return;
    }
    if !free(w, IVec3::new(cx + hw + 1, g, cz - 3), IVec3::new(exit_x + 2, g + 12, cz + 4)) {
        return;
    }
    // Coque en béton, puis l'intérieur vidé.
    w.fill(IVec3::new(cx - hw - 1, floor - 1, cz - hd - 1), IVec3::new(cx + hw + 2, top + 1, cz + hd + 2), CONCRETE_DARK);
    w.empty(IVec3::new(cx - hw, floor, cz - hd), IVec3::new(cx + hw + 1, top, cz + hd + 1));
    // Plafonniers.
    let mut x = cx - hw + 2;
    while x < cx + hw - 1 {
        w.fill(IVec3::new(x, top - 1, cz - 1), IVec3::new(x + 2, top, cz + 1), LAMP);
        x += 6;
    }
    // Lits superposés et caisses le long des murs.
    let mut z = cz - hd + 1;
    while z + 2 <= cz + hd {
        w.fill(IVec3::new(cx - hw, floor, z), IVec3::new(cx - hw + 4, floor + 1, z + 2), FURN_METAL);
        w.fill(IVec3::new(cx - hw, floor + 1, z), IVec3::new(cx - hw + 4, floor + 2, z + 2), BLUE);
        z += 4;
    }
    for _ in 0..rng.range(2, 5) {
        let at = IVec3::new(rng.range(cx - hw + 6, cx + hw - 2), floor, rng.range(cz - hd, cz + hd - 1));
        if free(w, at, at + IVec3::new(2, 2, 2)) {
            w.fill(at, at + IVec3::new(2, 2, 2), FURN_WOOD);
        }
    }
    // Porte et escalier qui remonte vers +X jusqu'à la surface.
    w.empty(IVec3::new(cx + hw + 1, floor, cz - 1), IVec3::new(cx + hw + 2, floor + 4, cz + 2));
    for k in 0..stairs {
        let x = cx + hw + 2 + k;
        let step = floor + (k * (g - floor)) / stairs; // marche (niveau où l'on marche)
        w.fill(IVec3::new(x, floor - 1, cz - 2), IVec3::new(x + 1, step, cz + 3), CONCRETE_DARK);
        w.empty(IVec3::new(x, step, cz - 1), IVec3::new(x + 1, (step + 5).min(g), cz + 2));
        // Murets de part et d'autre, jusqu'au-dessus du sol (rambarde).
        w.fill(IVec3::new(x, step, cz - 2), IVec3::new(x + 1, g + 1, cz - 1), CONCRETE_DARK);
        w.fill(IVec3::new(x, step, cz + 2), IVec3::new(x + 1, g + 1, cz + 3), CONCRETE_DARK);
        // Plafond de la descente tant qu'on est sous terre.
        if step + 5 < g {
            w.fill(IVec3::new(x, step + 5, cz - 1), IVec3::new(x + 1, g, cz + 2), CONCRETE_DARK);
        }
    }
    // Muret au fond de la trémie, côté bunker.
    w.fill(IVec3::new(cx + hw + 1, g, cz - 2), IVec3::new(cx + hw + 2, g + 1, cz + 3), CONCRETE_DARK);
}

fn park(w: &mut VoxelWorld, rng: &mut MapRng, min: IVec3, max: IVec3) {
    // Allée en croix.
    let mx = (min.x + max.x) / 2;
    let mz = (min.z + max.z) / 2;
    w.fill(IVec3::new(mx - 2, min.y - 1, min.z), IVec3::new(mx + 2, min.y, max.z), DIRT);
    w.fill(IVec3::new(min.x, min.y - 1, mz - 2), IVec3::new(max.x, min.y, mz + 2), DIRT);
    // Fontaine.
    w.fill(IVec3::new(mx - 4, min.y, mz - 4), IVec3::new(mx + 4, min.y + 2, mz + 4), CONCRETE);
    w.empty(IVec3::new(mx - 3, min.y + 1, mz - 3), IVec3::new(mx + 3, min.y + 2, mz + 3));
    w.fill(IVec3::new(mx - 1, min.y, mz - 1), IVec3::new(mx + 1, min.y + 5, mz + 1), CONCRETE);
    for _ in 0..rng.range(5, 10) {
        let x = rng.range(min.x + 4, max.x - 4);
        let z = rng.range(min.z + 4, max.z - 4);
        if (x - mx).abs() < 7 || (z - mz).abs() < 7 {
            continue;
        }
        tree(w, rng, IVec3::new(x, min.y, z));
    }
    // Bancs.
    for (x, z) in [(mx - 8, mz + 3), (mx + 5, mz - 5)] {
        w.fill(IVec3::new(x, min.y, z), IVec3::new(x + 4, min.y + 1, z + 1), WOOD);
    }    if rng.chance(0.45) {
        bunker(w, rng, min, max);
    }
}

fn parking(w: &mut VoxelWorld, rng: &mut MapRng, min: IVec3, max: IVec3) {
    w.fill(IVec3::new(min.x, min.y - 1, min.z), IVec3::new(max.x, min.y, max.z), ASPHALT);
    let mut x = min.x + 2;
    while x + 6 < max.x {
        // Marquage.
        w.fill(IVec3::new(x, min.y - 1, min.z + 2), IVec3::new(x + 1, min.y, min.z + 14), PAINT);
        w.fill(IVec3::new(x, min.y - 1, max.z - 14), IVec3::new(x + 1, min.y, max.z - 2), PAINT);
        if rng.chance(0.6) {
            car(w, rng, IVec3::new(x + 1, min.y, min.z + 3));
        }
        if rng.chance(0.6) {
            car(w, rng, IVec3::new(x + 1, min.y, max.z - 14));
        }
        x += 7;
    }
    if rng.chance(0.3) {
        bunker(w, rng, min, max);
    }
}

fn construction(w: &mut VoxelWorld, rng: &mut MapRng, min: IVec3, max: IVec3) {
    w.fill(IVec3::new(min.x, min.y - 1, min.z), IVec3::new(max.x, min.y, max.z), DIRT);
    // Squelette de béton : dalles et poteaux.
    let top = min.y + rng.range(2, 4) * 9;
    let (a, b) = (IVec3::new(min.x + 4, min.y, min.z + 4), IVec3::new(max.x - 4, top, max.z - 4));
    let mut y = a.y + 9;
    while y <= b.y {
        w.fill(IVec3::new(a.x, y - 1, a.z), IVec3::new(b.x, y, b.z), CONCRETE);
        y += 9;
    }
    let mut x = a.x;
    while x < b.x {
        let mut z = a.z;
        while z < b.z {
            w.fill(IVec3::new(x, a.y, z), IVec3::new(x + 2, b.y, z + 2), CONCRETE_DARK);
            z += 8;
        }
        x += 8;
    }
    // Rampe d'accès au premier niveau.
    for step in 0..8 {
        w.fill(
            IVec3::new(a.x - 3, a.y, a.z + 2 + step * 2),
            IVec3::new(a.x, a.y + step + 1, a.z + 4 + step * 2),
            WOOD,
        );
    }
    for _ in 0..rng.range(2, 4) {
        let x = rng.range(min.x + 2, max.x - 6);
        crate_stack(w, rng, IVec3::new(x, min.y, max.z - 5));
    }
    container(w, rng, IVec3::new(max.x - 8, min.y, min.z));
}

/// Arbre : tronc + boule de feuillage.
fn tree(w: &mut VoxelWorld, rng: &mut MapRng, at: IVec3) {
    let h = rng.range(6, 10);
    w.fill(at, at + IVec3::new(1, h, 1), WOOD);
    let r = rng.range(3, 4);
    let c = at + IVec3::new(0, h, 0);
    for x in -r..=r {
        for y in -r..=r {
            for z in -r..=r {
                if x * x + y * y + z * z <= r * r + 1 {
                    let p = c + IVec3::new(x, y, z);
                    if p.y >= GROUND && !w.is_solid(p) {
                        w.set(p, true, LEAVES);
                    }
                }
            }
        }
    }
}

/// Pile de caisses en bois.
fn crate_stack(w: &mut VoxelWorld, rng: &mut MapRng, at: IVec3) {
    let n = rng.range(1, 3);
    for i in 0..n {
        let o = at + IVec3::new(rng.range(0, 1), i * 4, rng.range(0, 1));
        w.fill(o, o + IVec3::splat(4), WOOD);
        // Bords plus sombres.
        w.fill(o + IVec3::new(0, 3, 0), o + IVec3::new(4, 4, 1), CONCRETE_DARK);
    }
}

/// Conteneur maritime ouvert d'un côté.
fn container(w: &mut VoxelWorld, rng: &mut MapRng, at: IVec3) {
    let m = rng.pick(&[ROOF, METAL, PAINT]);
    let (a, b) = (at, at + IVec3::new(6, 6, 15));
    w.fill(a, b, m);
    w.empty(a + IVec3::new(1, 1, 1), b - IVec3::new(1, 1, 0));
}

/// Voiture : caisse, habitacle vitré, roues.
fn car(w: &mut VoxelWorld, rng: &mut MapRng, at: IVec3) {
    let color = rng.pick(&[ROOF, METAL, PAINT, PLASTER, CONCRETE_DARK]);
    w.cars.push(at);
    // Roues.
    for (x, z) in [(0, 1), (4, 1), (0, 8), (4, 8)] {
        w.fill(at + IVec3::new(x, 0, z), at + IVec3::new(x + 1, 2, z + 2), CONCRETE_DARK);
    }
    w.fill(at + IVec3::new(0, 1, 0), at + IVec3::new(5, 3, 11), color);
    w.fill(at + IVec3::new(0, 3, 3), at + IVec3::new(5, 5, 8), GLASS);
    w.fill(at + IVec3::new(1, 3, 4), at + IVec3::new(4, 5, 7), color);
    w.fill(at + IVec3::new(0, 5, 3), at + IVec3::new(5, 6, 8), color);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le spawn serveur doit tomber **sur un trottoir**, en lieu de points
    /// libres : c'est ce qui manquait avant (tout le monde réapparaissait au
    /// carrefour central).
    #[test]
    fn random_sidewalk_lands_on_a_sidewalk_and_is_free() {
        let mut w = VoxelWorld::with_blocks(4);
        w.rebuild(1234);
        for salt in 0..8u64 {
            let Some(p) = random_sidewalk(&w, salt) else {
                continue;
            };
            // Bien dans la carte, et posé sur du béton au niveau du trottoir.
            assert!(w.in_bounds_xz(p), "hors carte : {p:?}");
            let below = w.world_to_cell(Vec3::new(p.x, p.y - VOXEL * 0.5, p.z));
            assert_eq!(w.material(below), CONCRETE, "pas sur un trottoir : {p:?}");
            assert!(below.y == GROUND, "pas à hauteur de trottoir : {p:?}");
            // De la place pour un joueur debout, des pieds à la tête.
            let half = Vec3::new(0.3, crate::player::HEIGHT * 0.5, 0.3);
            let center = Vec3::new(p.x, p.y + half.y, p.z);
            assert!(!w.overlaps(center - half, center + half), "joueur coincé : {p:?}");
        }
    }

    /// Aucune lampe ne flotte : chaque voxel lumineux est accroché à
    /// quelque chose au-dessus (plafond ou bras de lampadaire).
    #[test]
    fn lamps_hang_from_something() {
        let mut w = VoxelWorld::with_blocks(4);
        w.rebuild(1234);
        let lamp = |c: IVec3| w.is_solid(c) && w.material(c) == LAMP;
        let dirs = [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z];
        let mut seen = std::collections::HashSet::new();
        let (mut total, mut floating) = (0, 0);
        for x in 1..w.dims.x - 1 {
            for y in 1..w.dims.y - 1 {
                for z in 1..w.dims.z - 1 {
                    let c = IVec3::new(x, y, z);
                    if !lamp(c) || !seen.insert(c) {
                        continue;
                    }
                    // Groupe de voxels lumineux : accroché s'il touche autre chose.
                    let mut stack = vec![c];
                    let mut held = false;
                    let mut size = 0;
                    while let Some(p) = stack.pop() {
                        size += 1;
                        for d in dirs {
                            let n = p + d;
                            if lamp(n) {
                                if seen.insert(n) {
                                    stack.push(n);
                                }
                            } else if w.is_solid(n) {
                                held = true;
                            }
                        }
                    }
                    total += size;
                    if !held {
                        floating += size;
                    }
                }
            }
        }
        assert!(total > 50, "lampes : {total}");
        assert_eq!(floating, 0, "lampes en l'air : {floating} sur {total}");
    }

    /// Toute la ville générée tient au sol : aucun bloc isolé en l'air.
    #[test]
    fn city_is_connected_to_ground() {
        let mut w = VoxelWorld::with_blocks(4);
        w.rebuild(1234);
        let d = w.dims;
        let idx = |c: IVec3| ((c.y * d.z + c.z) * d.x + c.x) as usize;
        let mut reached = vec![false; (d.x * d.y * d.z) as usize];
        let mut stack = Vec::new();
        for x in 0..d.x {
            for z in 0..d.z {
                let c = IVec3::new(x, 0, z);
                if w.is_solid(c) {
                    reached[idx(c)] = true;
                    stack.push(c);
                }
            }
        }
        let dirs = [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z];
        while let Some(p) = stack.pop() {
            for o in dirs {
                let n = p + o;
                if n.cmpge(IVec3::ZERO).all() && n.cmplt(d).all() && !reached[idx(n)] && w.is_solid(n) {
                    reached[idx(n)] = true;
                    stack.push(n);
                }
            }
        }
        let mut lost: std::collections::BTreeMap<u8, usize> = Default::default();
        let mut example = None;
        for y in 0..d.y {
            for z in 0..d.z {
                for x in 0..d.x {
                    let c = IVec3::new(x, y, z);
                    if w.is_solid(c) && !reached[idx(c)] {
                        *lost.entry(w.material(c)).or_default() += 1;
                        example.get_or_insert(c);
                    }
                }
            }
        }
        assert!(lost.is_empty(), "blocs en l'air (matériau → nombre) : {lost:?}, ex. {example:?}");
    }

    /// Des bunkers existent : salles vides sous le sol, éclairées.
    #[test]
    fn bunkers_are_generated() {
        let mut w = VoxelWorld::with_blocks(8);
        w.rebuild(77);
        let mut rooms = 0;
        for x in (2..w.dims.x - 2).step_by(3) {
            for z in (2..w.dims.z - 2).step_by(3) {
                let c = IVec3::new(x, GROUND - 9, z);
                if !w.is_solid(c) && w.is_solid(c - IVec3::Y * 3) && w.is_solid(c + IVec3::Y * 6) {
                    rooms += 1;
                }
            }
        }
        assert!(rooms > 10, "cellules de bunker trouvées : {rooms}");
    }

    /// Deux joueurs ne doivent pas réapparaître au même endroit.
    #[test]
    fn different_players_get_different_sidewalks() {
        let mut w = VoxelWorld::with_blocks(4);
        w.rebuild(1234);
        let spots: Vec<Vec3> = (0..8u64).filter_map(|s| random_sidewalk(&w, s)).collect();
        assert!(spots.len() >= 2, "pas assez de trottoirs trouvés : {}", spots.len());
        for (i, a) in spots.iter().enumerate() {
            for b in &spots[i + 1..] {
                assert!(a.distance(*b) > 1.0, "deux joueurs au même point : {a:?}");
            }
        }
    }

    /// Le même joueur, la même ville : toujours le même trottoir (les
    /// sauvegardes et les réapparitions doivent être stables).
    #[test]
    fn sidewalk_is_stable_for_a_given_seed() {
        let mut w = VoxelWorld::with_blocks(4);
        w.rebuild(99);
        let a = random_sidewalk(&w, 3);
        let b = random_sidewalk(&w, 3);
        assert_eq!(a, b);
    }
}
