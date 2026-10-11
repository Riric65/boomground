//! Génération de la carte : sol, route, bâtiments, caisses — **tout est
//! destructible** (une seule grille de voxels).

use bevy::prelude::*;

use crate::world::mat::*;
use crate::world::VoxelWorld;

/// Découpe de la carte en cases de 8 × 8 cellules.
const Q: i32 = 8;

/// Épaisseur des murs, en voxels.
const WALL: i32 = 2;

/// Construit toute la carte.
pub fn generate(w: &mut VoxelWorld) {
    let d = w.dims;

    // --- Sol : terre everywhere, herbe au-dessus, route et dalle béton ---
    w.fill(IVec3::new(0, 0, 0), IVec3::new(d.x, 1, d.z), DIRT);
    w.fill(IVec3::new(0, 1, 0), IVec3::new(d.x, 3, d.z), GRASS);

    // Route (asphalte) le long de Z, au centre du terrain.
    w.fill(IVec3::new(52, 1, 0), IVec3::new(76, 3, d.z), ASPHALT);

    // Trottoirs de part et d'autre de la route.
    w.fill(IVec3::new(48, 1, 0), IVec3::new(52, 3, d.z), CONCRETE);
    w.fill(IVec3::new(76, 1, 0), IVec3::new(80, 3, d.z), CONCRETE);

    // Dalle devant le mur de briques.
    w.fill(IVec3::new(40, 1, 48), IVec3::new(88, 3, 76), CONCRETE);

    // --- Bâtiments (murs creux, sols, toits, ouvertures) ---

    building(w, IVec3::new(10, 3, 8), IVec3::new(44, 34, 42), BRICK, 2, true);
    building(w, IVec3::new(84, 3, 10), IVec3::new(118, 28, 44), METAL, 2, false);
    building(w, IVec3::new(8, 3, 84), IVec3::new(40, 26, 116), PLASTER, 2, true);
    building(w, IVec3::new(86, 3, 78), IVec3::new(118, 32, 116), BRICK_DARK, 2, true);

    // --- Le mur de briques du début (poteaux de test) ---
    w.fill(IVec3::new(48, 3, 58), IVec3::new(80, 23, 61), BRICK);
    // Fenêtres du mur.
    for (x0, x1) in [(53, 59), (69, 75)] {
        for x in x0..x1 {
            for y in 10..16 {
                w.set(IVec3::new(x, y, 58), false, 0);
                w.set(IVec3::new(x, y, 60), false, 0);
            }
        }
    }
    // Un pilier de chaque côté.
    w.fill(IVec3::new(44, 3, 56), IVec3::new(48, 25, 62), CONCRETE);
    w.fill(IVec3::new(80, 3, 56), IVec3::new(84, 25, 62), CONCRETE);

    // --- Caisses en bois ---
    for (x, z) in [
        (46, 40),
        (50, 44),
        (86, 40),
        (82, 52),
        (44, 74),
        (92, 70),
        (60, 30),
        (66, 96),
    ] {
        w.fill(IVec3::new(x, 3, z), IVec3::new(x + Q / 2, 3 + Q, z + Q), WOOD);
        w.fill(
            IVec3::new(x + 1, 3 + Q, z + 1),
            IVec3::new(x + Q / 2 - 1, 3 + Q + Q / 2, z + Q - 1),
            WOOD,
        );
    }

    // --- Muret de bord de terrain ---
    for (a, b) in [
        (IVec3::new(0, 3, 0), IVec3::new(4, 8, d.z)),
        (IVec3::new(d.x - 4, 3, 0), IVec3::new(d.x, 8, d.z)),
        (IVec3::new(0, 3, 0), IVec3::new(d.x, 8, 4)),
        (IVec3::new(0, 3, d.z - 4), IVec3::new(d.x, 8, d.z)),
    ] {
        w.fill(a, b, CONCRETE_DARK);
    }
}

/// Un bâtiment : murs, sols intermédiaires, toit, fenêtres et porte.
///
/// Les murs font deux voxels d'épaisseur : avec un seul, un bâtiment dont on
/// a détruit la moitié ressemble à du carton.
fn building(
    w: &mut VoxelWorld,
    min: IVec3,
    max: IVec3,
    material: u8,
    storeys: i32,
    windows: bool,
) {
    let height = max.y - min.y;

    // Murs (avec ouvertures).
    for (y, window_rows) in (0..height).map(|y| (y, y % 6 == 3)) {
        for z in min.z..max.z {
            for x in min.x..max.x {
                let on_wall = x < min.x + WALL
                    || x >= max.x - WALL
                    || z < min.z + WALL
                    || z >= max.z - WALL;
                if !on_wall {
                    continue;
                }
                let cell = IVec3::new(x, min.y + y, z);
                // Porte au milieu d'un mur.
                let door = y < 5
                    && ((x < min.x + WALL || x >= max.x - WALL) && z == (min.z + max.z) / 2 - 1)
                    || y < 5
                        && ((z < min.z + WALL || z >= max.z - WALL) && x == (min.x + max.x) / 2 - 1);
                if door {
                    continue;
                }
                // Fenêtres.
                if windows && window_rows && y > 5 && y < height - 4 {
                    let along = if x < min.x + WALL || x >= max.x - WALL {
                        (z - min.z) % 8
                    } else {
                        (x - min.x) % 8
                    };
                    if (2..6).contains(&along) {
                        continue;
                    }
                }
                // Colonnes briques plus sombres pour le relief.
                let m = if material == BRICK && (x + z + y) % 9 == 0 {
                    BRICK_DARK
                } else {
                    material
                };
                w.set(cell, true, m);
            }
        }
    }

    // Sols intermédiaires + toit.
    let mut y = min.y + 10;
    while y < max.y - 2 {
        w.fill(
            IVec3::new(min.x, y, min.z),
            IVec3::new(max.x, y + 1, max.z),
            if material == BRICK { MORTAR } else { CONCRETE_DARK },
        );
        y += 10;
    }
    w.fill(
        IVec3::new(min.x - 1, max.y - 2, min.z - 1),
        IVec3::new(max.x + 1, max.y, max.z + 1),
        if material == METAL { METAL } else { CONCRETE_DARK },
    );
    let _ = storeys;
}