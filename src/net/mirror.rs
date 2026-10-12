//! Reflet côté client de la simulation du serveur : gravats et corps
//! rigides. Le client ne simule rien : il crée les objets annoncés, les
//! déplace vers les poses reçues (avec lissage) et les supprime.

use std::collections::{BTreeMap, HashMap};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use crate::net::protocol::BlockInfo;
use crate::rubble::Chunk;
use crate::sim::BodyGrid;
use crate::vmesh::{self, VoxelMesh};
use crate::world::VoxelAssets;

/// Bloc de gravats reflété.
#[derive(Component)]
pub struct NetBlock {
    pub target: Vec3,
    pub yaw: f32,
}

/// Corps rigide reflété (la transformation de l'entité place la grille).
#[derive(Component)]
pub struct NetBody {
    pub com: Vec3,
    pub target_pos: Vec3,
    pub target_rot: Quat,
    pub rot: Quat,
    pub pos: Vec3,
}

struct Partial {
    dims: [u16; 3],
    com: Vec3,
    total: u16,
    parts: BTreeMap<u16, Vec<u8>>,
}

#[derive(Resource, Default)]
pub struct Mirror {
    blocks: HashMap<u32, Entity>,
    bodies: HashMap<u32, Entity>,
    partial: HashMap<u32, Partial>,
    /// Flammes affichées, par voxel.
    pub fires: HashMap<IVec3, Entity>,
}

impl Mirror {
    pub fn clear(&mut self, commands: &mut Commands) {
        for (_, e) in self.blocks.drain() {
            commands.entity(e).despawn();
        }
        for (_, e) in self.bodies.drain() {
            commands.entity(e).despawn();
        }
        self.partial.clear();
        for (_, e) in self.fires.drain() {
            commands.entity(e).despawn();
        }
    }

    pub fn body_entity(&self, id: u32) -> Option<Entity> {
        self.bodies.get(&id).copied()
    }

    pub fn add_blocks(&mut self, commands: &mut Commands, assets: &VoxelAssets, list: &[BlockInfo]) {
        for b in list {
            if let Some(old) = self.blocks.remove(&b.id) {
                commands.entity(old).despawn();
            }
            let size = b.half * 2.0;
            let e = commands
                .spawn((
                    NetBlock { target: b.pos, yaw: b.yaw },
                    Chunk {
                        vel: Vec3::ZERO,
                        spin: 0.0,
                        half: b.half,
                        sleeping: true,
                        rest: 0.0,
                        mass: 1.0,
                    },
                    Mesh3d(assets.chunk_cube.clone()),
                    MeshMaterial3d(assets.materials[b.mat as usize % assets.materials.len()].clone()),
                    Transform::from_translation(b.pos)
                        .with_rotation(Quat::from_rotation_y(b.yaw))
                        .with_scale(size),
                ))
                .id();
            self.blocks.insert(b.id, e);
        }
    }

    pub fn pose_blocks(&mut self, commands: &mut Commands, list: &[(u32, Vec3, f32)], q: &mut Query<&mut NetBlock>) {
        let _ = commands;
        for (id, pos, yaw) in list {
            if let Some(&e) = self.blocks.get(id) {
                if let Ok(mut b) = q.get_mut(e) {
                    b.target = *pos;
                    b.yaw = *yaw;
                }
            }
        }
    }

    pub fn remove_blocks(&mut self, commands: &mut Commands, ids: &[u32]) {
        for id in ids {
            if let Some(e) = self.blocks.remove(id) {
                commands.entity(e).despawn();
            }
        }
    }

    /// Reçoit un morceau de définition ; construit le corps quand il est
    /// complet.
    #[allow(clippy::too_many_arguments)]
    pub fn body_part(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        assets: &VoxelAssets,
        id: u32,
        dims: [u16; 3],
        com: Vec3,
        part: u16,
        total: u16,
        data: Vec<u8>,
    ) {
        let entry = self.partial.entry(id).or_insert_with(|| Partial {
            dims,
            com,
            total,
            parts: BTreeMap::new(),
        });
        // Une nouvelle définition remplace une définition inachevée.
        if entry.total != total || entry.dims != dims || part == 0 {
            *entry = Partial {
                dims,
                com,
                total,
                parts: BTreeMap::new(),
            };
        }
        entry.parts.insert(part, data);
        if entry.parts.len() < total as usize {
            return;
        }
        let Partial { dims, com, parts, .. } = self.partial.remove(&id).unwrap();
        let bytes: Vec<u8> = parts.into_values().flatten().collect();
        let dims3 = IVec3::new(dims[0] as i32, dims[1] as i32, dims[2] as i32);
        let Some(grid) = BodyGrid::decode_rle(dims3, &bytes) else { return };

        let previous = self.bodies.remove(&id);
        let (pos, rot) = match previous {
            Some(old) => {
                commands.entity(old).despawn();
                (Vec3::ZERO, Quat::IDENTITY)
            }
            None => (Vec3::ZERO, Quat::IDENTITY),
        };
        let _ = (pos, rot);
        let parent = commands
            .spawn((
                NetBody {
                    com,
                    target_pos: Vec3::ZERO,
                    target_rot: Quat::IDENTITY,
                    rot: Quat::IDENTITY,
                    pos: Vec3::ZERO,
                },
                Transform::IDENTITY,
                Visibility::default(),
            ))
            .id();
        let n = (dims3 + IVec3::splat(vmesh::CHUNK - 1)) / vmesh::CHUNK;
        for cx in 0..n.x {
            for cy in 0..n.y {
                for cz in 0..n.z {
                    let corner = IVec3::new(cx, cy, cz) * vmesh::CHUNK;
                    for pass in 0..3u8 {
                        let mesh = vmesh::build_pass(&grid, corner, pass);
                        if mesh.count_vertices() == 0 {
                            continue;
                        }
                        let material = match pass {
                            1 => assets.glass_material.clone(),
                            2 => assets.lamp_material.clone(),
                            _ => assets.voxel_material.clone(),
                        };
                        let mut e = commands.spawn((
                            VoxelMesh,
                            Mesh3d(meshes.add(mesh)),
                            MeshMaterial3d(material),
                            Transform::IDENTITY,
                        ));
                        if pass != 0 {
                            e.insert(NotShadowCaster);
                        }
                        let child = e.id();
                        commands.entity(parent).add_child(child);
                    }
                }
            }
        }
        self.bodies.insert(id, parent);
    }

    pub fn pose_bodies(&mut self, list: &[(u32, Vec3, [f32; 4])], q: &mut Query<&mut NetBody>) {
        for (id, pos, rot) in list {
            if let Some(&e) = self.bodies.get(id) {
                if let Ok(mut b) = q.get_mut(e) {
                    b.target_pos = *pos;
                    b.target_rot = Quat::from_array(*rot).normalize();
                }
            }
        }
    }

    pub fn remove_body(&mut self, commands: &mut Commands, id: u32) {
        self.partial.remove(&id);
        if let Some(e) = self.bodies.remove(&id) {
            commands.entity(e).despawn();
        }
    }
}

/// Lissage vers les poses reçues (20 par seconde).
pub fn smooth(
    time: Res<Time>,
    mut blocks: Query<(&NetBlock, &mut Transform), Without<NetBody>>,
    mut bodies: Query<(&mut NetBody, &mut Transform), Without<NetBlock>>,
) {
    let k = 1.0 - (-time.delta_secs() * 22.0).exp();
    for (b, mut t) in &mut blocks {
        if t.translation.distance_squared(b.target) > 1e-6 {
            t.translation = t.translation.lerp(b.target, k);
        }
        t.rotation = t.rotation.slerp(Quat::from_rotation_y(b.yaw), k);
    }
    for (mut b, mut t) in &mut bodies {
        b.pos = b.pos.lerp(b.target_pos, k);
        b.rot = b.rot.slerp(b.target_rot, k);
        t.rotation = b.rot;
        t.translation = b.pos - b.rot * b.com;
    }
}
