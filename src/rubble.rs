//! Gravats : quand on casse un voxel, il ne disparaît pas — il tombe.
//!
//! Les voxels détruits sont regroupés par blocs de `CHUNK` voxels de côté.
//! Un bloc qui n'existe pas encore est créé, puis **rétrécit** à chaque nouveau
//! voxel cassé : un bloc peut donc passer de 8 voxels à 1 sans disparaître
//! brutalement. Les blocs tombent, rebondissent, se cognent entre eux et
//! dorment une fois posés (ils restent définitivement à leur place).

use std::collections::HashMap;

use bevy::prelude::*;

use crate::debris::Rng;
use crate::world::{VoxelAssets, VoxelWorld, VOXEL};

/// Un bloc de gravats fait `CHUNK` voxels de côté (2 = 0,8 m).
pub const CHUNK: i32 = 2;
/// Nombre maximal de blocs simultanés (sécurité pour les très grosses destructions).
pub const MAX_CHUNKS: usize = 420;
const GRAVITY: f32 = 22.0;
/// Grille de collision bloc/bloc.
const CELL: f32 = 1.2;

/// Physique d'un bloc.
#[derive(Component)]
pub struct Chunk {
    pub vel: Vec3,
    pub spin: Vec3,
    /// Demi-taille courante (les blocs ne sont pas tout à fait cubiques).
    pub half: Vec3,
    pub sleeping: bool,
    /// Temps passé à l'arrêt (pour s'endormir).
    pub rest: f32,
}

#[derive(Clone, Copy)]
pub struct ChunkState {
    pub entity: Entity,
    /// Sous-voxels encore vivants dans le bloc.
    pub alive: f32,
}

#[derive(Resource, Default)]
pub struct Chunks {
    /// Blocs connus : clef de zone → état.
    pub map: HashMap<IVec3, ChunkState>,
}

impl Chunks {
    pub fn len(&self) -> usize {
        self.map.len()
    }
}

/// Coin bas du bloc de gravats contenant cette cellule.
pub fn chunk_key(cell: IVec3) -> IVec3 {
    (cell / CHUNK) * CHUNK
}

/// Un bloc existe tant qu'il reste au moins un voxel dans sa zone.
/// Règle partagée par le client et le serveur (pas de physique partagée).
pub fn zone_has_voxels(world: &VoxelWorld, key: IVec3) -> bool {
    for dx in 0..CHUNK {
        for dy in 0..CHUNK {
            for dz in 0..CHUNK {
                if world.is_solid(key + IVec3::new(dx, dy, dz)) {
                    return true;
                }
            }
        }
    }
    false
}

/// Centre monde de la zone d'un bloc (pour l'apparition réseau).
pub fn zone_center(world: &VoxelWorld, key: IVec3) -> Vec3 {
    let mut min = IVec3::new(i32::MAX, i32::MAX, i32::MAX);
    let mut max = IVec3::new(i32::MIN, i32::MIN, i32::MIN);
    for dx in 0..CHUNK {
        for dy in 0..CHUNK {
            for dz in 0..CHUNK {
                let c = key + IVec3::new(dx, dy, dz);
                min = min.min(c);
                max = max.max(c);
            }
        }
    }
    (world.cell_min(min) + world.cell_max(max)) * 0.5
}

/// Matériau dominant de la zone (pour l'apparition réseau).
pub fn zone_material(world: &VoxelWorld, key: IVec3) -> u8 {
    for dx in 0..CHUNK {
        for dy in 0..CHUNK {
            for dz in 0..CHUNK {
                let c = key + IVec3::new(dx, dy, dz);
                if world.is_solid(c) {
                    return world.material(c);
                }
            }
        }
    }
    0
}

/// Fait apparaître le bloc décrit par le serveur (et l'enregistre).
pub fn spawn_from_net(
    chunks: &mut Chunks,
    commands: &mut Commands,
    assets: &VoxelAssets,
    key: IVec3,
    at: Vec3,
    material: u8,
) {
    if chunks.map.contains_key(&key) || chunks.map.len() >= MAX_CHUNKS {
        return;
    }
    let size = chunk_size(1.0);
    let entity = commands
        .spawn((
            Chunk {
                vel: Vec3::ZERO,
                spin: Vec3::ZERO,
                half: Vec3::splat(size * 0.5),
                sleeping: false,
                rest: 0.0,
            },
            Mesh3d(assets.chunk_cube.clone()),
            MeshMaterial3d(assets.materials[material as usize].clone()),
            Transform::from_translation(at).with_scale(Vec3::splat(size)),
        ))
        .id();
    chunks.map.insert(key, ChunkState { entity, alive: 1.0 });
}

/// Taille visuelle d'un bloc selon le nombre de sous-voxels restants.
fn chunk_size(alive: f32) -> f32 {
    let total = (CHUNK * CHUNK * CHUNK) as f32;
    (VOXEL * CHUNK as f32) * (alive / total).powf(1.0 / 3.0)
}

/// À appeler pour **chaque** voxel détruit : crée ou rétrécit son bloc.
#[allow(clippy::too_many_arguments)]
pub fn break_voxel(
    chunks: &mut Chunks,
    commands: &mut Commands,
    assets: &VoxelAssets,
    world: &VoxelWorld,
    cell: IVec3,
    impact: Vec3,
    rng: &mut Rng,
) {
    let key = chunk_key(cell);

    // Ce qui reste de vivant dans le bloc (la destruction a déjà eu lieu).
    let mut alive = 0u32;
    let mut mat = 0u8;
    for dx in 0..CHUNK {
        for dy in 0..CHUNK {
            for dz in 0..CHUNK {
                let c = key + IVec3::new(dx, dy, dz);
                if world.is_solid(c) {
                    alive += 1;
                    mat = world.material(c);
                }
            }
        }
    }

    if alive == 0 {
        if let Some(state) = chunks.map.remove(&key) {
            commands.entity(state.entity).despawn();
        }
        return;
    }

    if let Some(state) = chunks.map.get_mut(&key) {
        state.alive = alive as f32;
        return;
    }

    if chunks.map.len() >= MAX_CHUNKS {
        return;
    }

    // Nouveau bloc : il apparaît là où était le voxel cassé.
    let size = chunk_size(alive as f32);
    let half = Vec3::splat(size * 0.5);
    let away = (world.cell_center(cell) - impact).normalize_or_zero();
    let vel = away * (1.5 + rng.f32() * 2.5)
        + Vec3::new(rng.signed(), rng.signed(), rng.signed()) * 1.2
        + Vec3::Y * (0.5 + rng.f32() * 1.5);

    let entity = commands
        .spawn((
            Chunk {
                vel,
                spin: Vec3::new(rng.signed() * 4.0, rng.signed() * 4.0, rng.signed() * 4.0),
                half,
                sleeping: false,
                rest: 0.0,
            },
            Mesh3d(assets.chunk_cube.clone()),
            MeshMaterial3d(assets.materials[mat as usize].clone()),
            Transform::from_translation(world.cell_center(cell))
                .with_scale(Vec3::splat(size))
                .with_rotation(Quat::from_euler(
                    EulerRot::YXZ,
                    rng.signed() * 0.4,
                    rng.signed() * 0.4,
                    rng.signed() * 0.4,
                )),
        ))
        .id();

    chunks.map.insert(key, ChunkState { entity, alive: alive as f32 });
}

/// Recalcule la taille visuelle des blocs après un rétrécissement.
pub fn sync_scales(
    chunks: Res<Chunks>,
    mut query: Query<(&mut Transform, &mut Chunk)>,
) {
    for state in chunks.map.values() {
        let size = chunk_size(state.alive);
        let Ok((mut transform, mut chunk)) = query.get_mut(state.entity) else {
            continue;
        };
        if (transform.scale.x - size).abs() > 0.005 {
            transform.scale = Vec3::splat(size);
            chunk.half = Vec3::splat(size * 0.5);
        }
    }
}

/// Gravité, sol, voxels et blocs voisins.
pub fn update(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    chunks: Res<Chunks>,
    mut query: Query<(Entity, &mut Transform, &mut Chunk)>,
) {
    let dt = time.delta_secs().min(0.05);

    // Instantané des blocs pour les collisions mutuelles.
    #[derive(Clone, Copy)]
    struct Snap {
        pos: Vec3,
        half: Vec3,
        sleeping: bool,
    }
    let mut snap: Vec<Snap> = Vec::with_capacity(chunks.len());
    let mut grid: HashMap<IVec3, Vec<usize>> = HashMap::new();
    for (entity, transform, chunk) in &query {
        let key = (transform.translation / CELL).floor().as_ivec3();
        let idx = snap.len();
        snap.push(Snap {
            pos: transform.translation,
            half: chunk.half,
            sleeping: chunk.sleeping,
        });
        grid.entry(key).or_default().push(idx);
        let _ = entity;
    }

    for (i, (_, mut transform, mut chunk)) in query.iter_mut().enumerate() {
        if chunk.sleeping {
            continue;
        }

        chunk.vel.y -= GRAVITY * dt;
        transform.translation += chunk.vel * dt;
        transform.rotate(Quat::from_euler(
            EulerRot::XYZ,
            chunk.spin.x * dt,
            chunk.spin.y * dt,
            chunk.spin.z * dt,
        ));

        // Filet de sécurité : très loin sous la carte (le sol a été détruit
        // sous le bloc), on le pose sur le plan y = 0 plutôt que de le
        // laisser tomber pour toujours.
        if transform.translation.y < chunk.half.y {
            transform.translation.y = chunk.half.y;
            chunk.vel.y = -chunk.vel.y * 0.2;
            chunk.vel.x *= 0.75;
            chunk.vel.z *= 0.75;
            chunk.spin *= 0.55;
        }

        // Contre les voxels encore debout (sol compris : pas de plan
        // imaginaire, le terrain EST fait de voxels).
        for axis in 0..3 {
            let delta = get(chunk.vel, axis) * dt;
            if delta != 0.0 {
                let mut p = transform.translation;
                if world.sweep(&mut p, chunk.half, axis, delta) {
                    set(&mut transform.translation, axis, get(p, axis));
                    set(&mut chunk.vel, axis, 0.0);
                }
            }
        }

        // Contre les blocs déjà posés (considérés comme statiques).
        let here = (transform.translation / CELL).floor().as_ivec3();
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(list) = grid.get(&(here + IVec3::new(dx, dy, dz))) else {
                        continue;
                    };
                    for &j in list {
                        if j == i {
                            continue;
                        }
                        let other = snap[j];
                        if !other.sleeping {
                            continue;
                        }
                        let d = transform.translation - other.pos;
                        let overlap = (chunk.half + other.half) - d.abs();
                        if overlap.min_element() > 0.0 {
                            // On repousse le bloc mobile vers le haut.
                            let push = overlap.min_element();
                            transform.translation.y += push;
                            chunk.vel.y = chunk.vel.y.max(0.0);
                            chunk.vel.x *= 0.6;
                            chunk.vel.z *= 0.6;
                        }
                    }
                }
            }
        }

        // Mise en sommeil.
        if chunk.vel.length() < 0.35 {
            chunk.rest += dt;
            if chunk.rest > 0.35 {
                chunk.sleeping = true;
                chunk.vel = Vec3::ZERO;
                chunk.spin = Vec3::ZERO;
            }
        } else {
            chunk.rest = 0.0;
        }
    }
}

fn get(v: Vec3, a: usize) -> f32 {
    match a {
        0 => v.x,
        1 => v.y,
        _ => v.z,
    }
}

fn set(v: &mut Vec3, a: usize, value: f32) {
    match a {
        0 => v.x = value,
        1 => v.y = value,
        _ => v.z = value,
    }
}