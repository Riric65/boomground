//! Débris : les petits cubes qui volent quand on casse un voxel.

use bevy::prelude::*;

use crate::world::{get_component, set_component, VoxelAssets, VoxelWorld};

/// Mini générateur pseudo-aléatoire (xorshift64), sans dépendance externe.
#[derive(Resource)]
pub struct Rng(u64);

impl Default for Rng {
    fn default() -> Self {
        Self(0x2545_F491_4F6C_DD1D)
    }
}

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Float dans `[0, 1)`.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Entier dans `[0, n)`.
    pub fn usize(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }

    /// Float dans `[-1, 1)`.
    pub fn signed(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }
}

#[derive(Component)]
pub struct Debris {
    pub vel: Vec3,
    pub spin: Vec3,
    pub life: f32,
    /// Demi-taille du débris (pour la collision).
    pub half: f32,
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_debris(
    commands: &mut Commands,
    assets: &VoxelAssets,
    rng: &mut Rng,
    pos: Vec3,
    mat: u8,
    vel: Vec3,
) {
    let scale = 0.45 + rng.f32() * 0.5;
    commands.spawn((
        Debris {
            vel,
            spin: Vec3::new(rng.signed() * 9.0, rng.signed() * 9.0, rng.signed() * 9.0),
            life: 1.6 + rng.f32() * 1.2,
            half: scale * 0.5,
        },
        Mesh3d(assets.debris_cube.clone()),
        MeshMaterial3d(assets.materials[mat as usize].clone()),
        Transform::from_translation(pos).with_scale(Vec3::splat(scale)),
    ));
}

/// Gravité, collision avec les voxels, frottements, disparition.
///
/// Les débris rebondissent sur la géométrie réelle (pas sur un plan imaginaire)
/// et perdent vite de l'énergie : ils finissent posés au lieu de rouler
/// indéfiniment.
pub fn update_debris(
    time: Res<Time>,
    world: Res<VoxelWorld>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut Transform, &mut Debris)>,
) {
    let dt = time.delta_secs().min(0.05);

    for (entity, mut transform, mut debris) in &mut query {
        // Gravité + frottement de l'air (légèreResistance).
        debris.vel.y -= 22.0 * dt;
        debris.vel *= 1.0 - (0.6 * dt).min(0.5);
        debris.spin *= 1.0 - (1.2 * dt).min(0.5);

        let mut pos = transform.translation;
        let half = Vec3::splat(debris.half);
        let mut contact = false;

        for axis in 0..3 {
            let delta = get_component(debris.vel, axis) * dt;
            if delta == 0.0 {
                continue;
            }
            if world.sweep(&mut pos, half, axis, delta) {
                contact = true;
                if axis == 1 {
                    // Rebond amorti : un débris ne « bondit » pas comme une
                    // balle, il ricoche et se pose.
                    debris.vel.y = -debris.vel.y * 0.28;
                    if debris.vel.y.abs() < 0.6 {
                        debris.vel.y = 0.0;
                    }
                } else {
                    let v = get_component(debris.vel, axis);
                    set_component(&mut debris.vel, axis, -v * 0.35);
                }
            }
        }

        // Filet de sécurité : très loin sous la carte, on fait tomber le
        // débris au lieu de le laisser tomber indéfiniment.
        if pos.y < -12.0 {
            pos.y = -12.0;
            debris.vel = Vec3::ZERO;
        }

        transform.translation = pos;
        transform.rotate(Quat::from_euler(
            EulerRot::XYZ,
            debris.spin.x * dt,
            debris.spin.y * dt,
            debris.spin.z * dt,
        ));

        // Au sol, on freine franchement : un peu de glissement, puis plus
        // rien du tout (comme un gravier qui s'arrête).
        if contact && debris.vel.y <= 0.0 {
            let friction = 1.0 - (5.5 * dt).min(1.0);
            debris.vel.x *= friction;
            debris.vel.z *= friction;
        }

        debris.life -= dt;
        if debris.life <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}
