//! Gravats côté client : de simples boîtes reflétant la simulation du serveur
//! (voir `net::mirror`). Le composant [`Chunk`] sert aux collisions du
//! joueur ; la physique elle-même est dans `sim`.

use bevy::prelude::*;

use crate::world::VoxelWorld;

/// Un bloc de gravats (position donnée par le serveur).
#[derive(Component)]
pub struct Chunk {
    pub vel: Vec3,
    pub spin: f32,
    /// Demi-taille.
    pub half: Vec3,
    pub sleeping: bool,
    pub rest: f32,
    pub mass: f32,
}

/// Boîte `(min, max)` d'un bloc.
pub fn aabb(transform: &Transform, chunk: &Chunk) -> (Vec3, Vec3) {
    (transform.translation - chunk.half, transform.translation + chunk.half)
}

/// Supprime tout le reflet de la simulation quand la carte est régénérée.
pub fn clear_on_rebuild(
    world: Res<VoxelWorld>,
    mut seen: Local<u32>,
    mut mirror: ResMut<crate::net::mirror::Mirror>,
    mut commands: Commands,
) {
    if *seen == world.generation {
        return;
    }
    *seen = world.generation;
    mirror.clear(&mut commands);
}
