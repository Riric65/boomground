//! Gravats : quand on casse des voxels, ils ne disparaissent pas — ils
//! tombent sous forme de blocs.
//!
//! Les voxels détruits d'un même coup sont regroupés par zones de 2×2×2 ;
//! chaque zone donne **un** bloc dont la taille est exactement la boîte des
//! voxels cassés (donc il apparaît dans le vide qu'il vient de créer, et non
//! plus dans le mur restant). Les blocs tombent, rebondissent, s'empilent,
//! puis dorment. Ils ne tournent qu'autour de l'axe vertical : leur boîte de
//! collision correspond donc à ce qu'on voit (avant, des cubes penchés
//! flottaient au-dessus du sol).

use std::collections::{HashMap, VecDeque};

use bevy::prelude::*;

use crate::debris::Rng;
use crate::world::{VoxelAssets, VoxelWorld};

/// Une zone de gravats fait `ZONE` voxels de côté (2 = 0,8 m).
pub const ZONE: i32 = 2;
/// Nombre maximal de blocs simultanés : au-delà, les plus anciens partent.
pub const MAX_BLOCKS: usize = 700;
/// Blocs créés au maximum par destruction (une méga-bombe casse des
/// dizaines de milliers de voxels).
const MAX_PER_HIT: usize = 160;
const GRAVITY: f32 = 22.0;
/// Grille de collision bloc/bloc.
const CELL: f32 = 1.0;

/// Physique d'un bloc.
#[derive(Component)]
pub struct Chunk {
    pub vel: Vec3,
    /// Rotation autour de Y (rad/s) pendant le vol.
    pub spin: f32,
    /// Demi-taille.
    pub half: Vec3,
    pub sleeping: bool,
    /// Temps passé presque immobile.
    pub rest: f32,
    /// Masse (volume × masse volumique, ~1 pour un bloc de brique de
    /// 2×2×2 voxels) : les lourds sont moins poussés et écrasent.
    pub mass: f32,
}

/// Blocs vivants, du plus ancien au plus récent.
#[derive(Resource, Default)]
pub struct Rubble {
    pub order: VecDeque<Entity>,
}

impl Rubble {
    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// Crée les blocs correspondant aux voxels `destroyed` (déjà vidés).
#[allow(clippy::too_many_arguments)]
pub fn spawn_rubble(
    rubble: &mut Rubble,
    commands: &mut Commands,
    assets: &VoxelAssets,
    world: &VoxelWorld,
    destroyed: &[IVec3],
    impact: Vec3,
    push: Vec3,
    rng: &mut Rng,
) {
    if destroyed.is_empty() {
        return;
    }
    // Regroupement par zone : boîte englobante + matériau dominant.
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
    // Sélection au hasard si la destruction est énorme (la plupart des
    // voxels d'une grosse explosion sont pulvérisés).
    if list.len() > MAX_PER_HIT {
        for i in 0..MAX_PER_HIT {
            let j = i + rng.usize(list.len() - i);
            list.swap(i, j);
        }
        list.truncate(MAX_PER_HIT);
    }

    for (lo, hi, material) in list {
        // Les vitres se brisent : pas de bloc.
        if material == crate::world::mat::GLASS {
            continue;
        }
        let min = world.cell_min(lo);
        let max = world.cell_max(hi);
        let center = (min + max) * 0.5;
        // Un peu plus petit que la cavité : pas de contact initial.
        let size = (max - min) * 0.9;
        let half = size * 0.5;

        let away = (center - impact).normalize_or_zero();
        let strength = push.length().max(1.0);
        let vel = away * (1.0 + rng.f32() * 1.5) * strength.sqrt()
            + push * (0.3 + rng.f32() * 0.4)
            + Vec3::new(rng.signed(), rng.f32() * 0.5, rng.signed()) * 1.2;

        let entity = commands
            .spawn((
                Chunk {
                    vel,
                    spin: rng.signed() * 5.0,
                    half,
                    sleeping: false,
                    rest: 0.0,
                    mass: size.x * size.y * size.z * crate::world::density(material),
                },
                Mesh3d(assets.chunk_cube.clone()),
                MeshMaterial3d(assets.materials[material as usize % assets.materials.len()].clone()),
                Transform::from_translation(center).with_scale(size),
            ))
            .id();
        rubble.order.push_back(entity);
    }

    // On garde un nombre raisonnable de blocs.
    while rubble.order.len() > MAX_BLOCKS {
        if let Some(old) = rubble.order.pop_front() {
            commands.entity(old).despawn();
        }
    }
}

/// Réveille les blocs proches d'une destruction (ils peuvent avoir perdu
/// leur support) et les pousse.
pub fn wake_near(query: &mut Query<(&Transform, &mut Chunk)>, at: Vec3, radius: f32, push: f32) {
    for (transform, mut chunk) in query.iter_mut() {
        let d = transform.translation - at;
        let dist = d.length();
        if dist < radius + 1.5 {
            chunk.sleeping = false;
            chunk.rest = 0.0;
            if push > 0.0 && dist < radius * 1.5 {
                let falloff = 1.0 - (dist / (radius * 1.5)).min(1.0);
                let light = (1.0 / chunk.mass.max(0.4)).min(2.0);
                chunk.vel += (d.normalize_or(Vec3::Y) * push * falloff + Vec3::Y * push * 0.4 * falloff) * light;
            }
        }
    }
}

/// Boîte `(min, max)` d'un bloc.
pub fn aabb(transform: &Transform, chunk: &Chunk) -> (Vec3, Vec3) {
    (transform.translation - chunk.half, transform.translation + chunk.half)
}

/// Supprime tous les gravats (régénération de la carte).
pub fn clear_on_rebuild(
    world: Res<VoxelWorld>,
    mut seen: Local<u32>,
    mut rubble: ResMut<Rubble>,
    mut commands: Commands,
) {
    if *seen == world.generation {
        return;
    }
    *seen = world.generation;
    for entity in rubble.order.drain(..) {
        commands.entity(entity).despawn();
    }
}

/// Gravité, voxels, blocs voisins, sommeil.
pub fn update(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    mut rubble: ResMut<Rubble>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut Transform, &mut Chunk)>,
) {
    let dt = time.delta_secs().min(0.05);

    // Instantané des blocs endormis pour les collisions mutuelles.
    let mut grid: HashMap<IVec3, Vec<(Entity, Vec3, Vec3)>> = HashMap::new();
    for (entity, transform, chunk) in &query {
        if !chunk.sleeping {
            continue;
        }
        let (min, max) = aabb(&transform, &chunk);
        let c0 = (min / CELL).floor().as_ivec3();
        let c1 = (max / CELL).floor().as_ivec3();
        for x in c0.x..=c1.x {
            for y in c0.y..=c1.y {
                for z in c0.z..=c1.z {
                    grid.entry(IVec3::new(x, y, z)).or_default().push((entity, min, max));
                }
            }
        }
    }

    let mut lost = Vec::new();
    for (entity, mut transform, mut chunk) in &mut query {
        if chunk.sleeping {
            continue;
        }

        chunk.vel.y -= GRAVITY * dt;
        let mut pos = transform.translation;

        // Les autres blocs posés, autour de la trajectoire.
        let reach = chunk.half + Vec3::splat(chunk.vel.length() * dt + 0.1);
        let c0 = ((pos - reach) / CELL).floor().as_ivec3();
        let c1 = ((pos + reach) / CELL).floor().as_ivec3();
        let mut boxes: Vec<(Vec3, Vec3)> = Vec::new();
        for x in c0.x..=c1.x {
            for y in c0.y..=c1.y {
                for z in c0.z..=c1.z {
                    if let Some(list) = grid.get(&IVec3::new(x, y, z)) {
                        for &(other, min, max) in list {
                            if other != entity && !boxes.contains(&(min, max)) {
                                boxes.push((min, max));
                            }
                        }
                    }
                }
            }
        }

        // Y d'abord : le contact avec le sol décide du frottement.
        let mut grounded = false;
        for axis in [1, 0, 2] {
            let delta = chunk.vel[axis] * dt;
            if delta == 0.0 {
                continue;
            }
            if world.sweep_with(&mut pos, chunk.half, axis, delta, &boxes) {
                if axis == 1 && chunk.vel.y < 0.0 {
                    grounded = true;
                    chunk.vel.y = -chunk.vel.y * 0.18;
                    if chunk.vel.y < 0.8 {
                        chunk.vel.y = 0.0;
                    }
                    chunk.vel.x *= 0.6;
                    chunk.vel.z *= 0.6;
                    chunk.spin *= 0.5;
                } else {
                    chunk.vel[axis] = -chunk.vel[axis] * 0.25;
                }
            }
        }

        if grounded {
            // Frottement au sol : un bloc glisse un peu puis s'arrête.
            let friction = 1.0 - (6.0 * dt).min(1.0);
            chunk.vel.x *= friction;
            chunk.vel.z *= friction;
        }

        transform.translation = pos;
        transform.rotate_y(chunk.spin * dt);

        // Tombé hors du monde.
        if pos.y < -8.0 {
            lost.push(entity);
            continue;
        }

        // Mise en sommeil.
        if grounded && chunk.vel.length() < 0.4 {
            chunk.rest += dt;
            if chunk.rest > 0.25 {
                chunk.sleeping = true;
                chunk.vel = Vec3::ZERO;
                chunk.spin = 0.0;
            }
        } else if !grounded {
            chunk.rest = 0.0;
        }
    }

    for entity in lost {
        commands.entity(entity).despawn();
        rubble.order.retain(|&e| e != entity);
    }
}

/// Les blocs endormis dont le support a été détruit retombent.
pub fn wake_unsupported(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    mut watch: Local<(u32, f32)>,
    mut query: Query<(&Transform, &mut Chunk)>,
) {
    // On ne vérifie que pendant les secondes qui suivent une destruction
    // (le temps que les blocs du dessus retombent en cascade).
    if watch.0 != world.alive {
        *watch = (world.alive, 2.5);
    }
    if watch.1 <= 0.0 {
        return;
    }
    watch.1 -= time.delta_secs();

    let blocks: Vec<(Vec3, Vec3)> = query
        .iter()
        .filter(|(_, c)| c.sleeping)
        .map(|(t, c)| aabb(t, c))
        .collect();

    for (transform, mut chunk) in &mut query {
        if !chunk.sleeping {
            continue;
        }
        let (min, max) = aabb(transform, &chunk);
        // Petite tranche juste sous le bloc.
        let below_min = Vec3::new(min.x + 0.02, min.y - 0.06, min.z + 0.02);
        let below_max = Vec3::new(max.x - 0.02, min.y + 0.01, max.z - 0.02);
        if world.overlaps(below_min, below_max) {
            continue;
        }
        let on_block = blocks.iter().any(|&(bmin, bmax)| {
            bmin.x < below_max.x
                && bmax.x > below_min.x
                && bmin.z < below_max.z
                && bmax.z > below_min.z
                && bmax.y >= below_min.y
                && bmax.y <= below_max.y + 0.05
                && bmin != min
        });
        if !on_block {
            chunk.sleeping = false;
            chunk.rest = 0.0;
        }
    }
}
